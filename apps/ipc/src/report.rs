//! 面板 F 的報表聚合。
//!
//! UI 規格 F.1：統計主體**只有使用者座位**（核心規格 5.0）。Bot 的結果
//! 不進入這裡的任何一個數字——它們只用來建立脈絡（誰是翻前攻擊者、
//! 使用者面對的是不是加注），不會成為分子或分母。
//!
//! # 為什麼聚合在 Rust 而不是前端
//!
//! F.3 的驗收寫得很直接：「UI 只呈現引擎與統計層結果，**不重算牌局
//! 邏輯**」。VPIP、C-bet 這些頻率看起來像是數一數就好，實際上每一個都
//! 要先知道「當下的下注門檻是多少」「誰是翻前攻擊者」——那是下注規則，
//! 複製到前端就等於有兩套規則，而且只有其中一套有測試。
//!
//! # 誠實揭露不是選配
//!
//! F.5 要求區間一律顯示、CI 跨 0 標「無法判定」、樣本不足另外標示。
//! 因此本模組的回傳型別裡**沒有裸點估計**：EV 一定帶 CI 與判定狀態，
//! 比例一定帶分子／分母與 Wilson 區間。UI 想只畫一個數字也畫不出來。

use std::collections::BTreeMap;

use poker_engine::betting::Action;
use poker_engine::hand::Street;
use poker_engine::position::{resolve, PositionLabel};
use poker_engine::rng::{Rng, RngDomain};
use poker_engine::stats::{
    estimate_block_length, moving_block_bootstrap, wilson, Estimate, Observation, Proportion,
    DEFAULT_RESAMPLES,
};
use poker_storage::codec::{HandRecord, PostKind};
use poker_storage::db::Store;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::handler::IpcError;

/// 逐位置切片的重抽次數。
///
/// 整體卡用 [`DEFAULT_RESAMPLES`]，切片降到這個數：重抽成本是
/// `次數 × 手數`，而切片最多 30 個（在桌人數 6–9 × 各自位置數）。
/// 全部用 2000 次會讓 10 萬手的報表卡在 F.4 的 500ms 門檻之外，
/// 而切片的區間本來就寬，重抽次數不是它的精度瓶頸。
const SLICE_RESAMPLES: usize = 500;

/// 一次讀進記憶體的手數。10 萬手的紀錄全部展開約數十 MB，
/// 分頁讀取讓峰值只跟這個常數有關
const PAGE: u64 = 4_096;

// ── DTO ─────────────────────────────────────────────────────────────

/// 比例指標。
///
/// 核心規格 5.3 要求比例一律顯示分子／分母：60% 在 3/5 與 600/1000 是
/// 完全不同的證據強度，只給百分比會讓兩者看起來一樣。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ProportionView {
    /// 顯示名稱，例如 `VPIP`
    pub label: String,
    /// 指標定義的一句話說明（UI 規格 F.2）
    pub definition: String,
    #[ts(type = "number")]
    pub numerator: u64,
    #[ts(type = "number")]
    pub denominator: u64,
    /// 點估計，0.0～1.0。分母為 0 時為 null（核心規格 5.4：顯示 N/A）
    #[ts(type = "number | null")]
    pub point: Option<f64>,
    /// Wilson 區間，0.0～1.0。分母為 0 時為 null
    #[ts(type = "number | null")]
    pub ci_low: Option<f64>,
    #[ts(type = "number | null")]
    pub ci_high: Option<f64>,
}

impl ProportionView {
    fn new(label: &str, definition: &str, p: Proportion) -> Self {
        let has_sample = p.denominator > 0;
        Self {
            label: label.to_owned(),
            definition: definition.to_owned(),
            numerator: p.numerator,
            denominator: p.denominator,
            point: p.point(),
            ci_low: has_sample.then_some(p.ci_low),
            ci_high: has_sample.then_some(p.ci_high),
        }
    }
}

/// bb/100 的區間估計，含 estimator 與可判定狀態。
///
/// 沒有「只有點估計」的形式：核心規格 5.3 要求 estimator 名稱與有效樣本
/// 必須顯示於報表，而 F.5 要求區間一律顯示。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct EstimateView {
    /// 點估計，bb/100
    pub point: f64,
    pub ci_low: f64,
    pub ci_high: f64,
    /// CI 半寬。F.5 的「此手數可分辨的最小差距」即取此值
    pub half_width: f64,
    pub estimator: String,
    /// 有效樣本，以 block 數計（核心規格 5.3）
    #[ts(type = "number")]
    pub effective_blocks: u64,
    #[ts(type = "number")]
    pub hands: u64,
    /// 可判定／本樣本無法判定優劣／樣本不足
    pub verdict: String,
}

impl From<Estimate> for EstimateView {
    fn from(e: Estimate) -> Self {
        Self {
            point: e.point,
            ci_low: e.ci_low,
            ci_high: e.ci_high,
            half_width: e.half_width(),
            estimator: e.estimator.as_str().to_owned(),
            effective_blocks: e.effective_blocks as u64,
            hands: e.hands as u64,
            verdict: e.verdict.as_str().to_owned(),
        }
    }
}

/// 資料範圍的脈絡（UI 規格 F.4 範圍選擇器）。
///
/// 核心規格 3.3：跨 engine 版本的 run 不得合併統計。因此版本不是裝飾，
/// 是判斷「這份數字能不能跟另一份比」的依據，必須跟著報表一起出現。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ReportScopeView {
    #[ts(type = "number")]
    pub run_id: i64,
    /// 範圍的顯示名稱。目前只有「本次 run」
    pub label: String,
    #[ts(type = "number")]
    pub hands: u64,
    /// 桌次數。block／cluster 是否足夠由它決定（核心規格 5.3）
    #[ts(type = "number")]
    pub instance_count: u64,
    pub players: u8,
    pub hero_seat: u8,
    #[ts(type = "number")]
    pub big_blind: u64,
    pub engine_version: String,
    pub baseline_version: String,
    /// 自身策略內容的名稱與版本
    pub hero_strategy: String,
    /// Bot 組合，逐座的人格名稱
    pub bot_personas: Vec<String>,
    /// 建立時間（Unix 秒）
    #[ts(type = "number")]
    pub created_at: i64,
    pub completed: bool,
}

/// 整體卡（UI 規格 F.4）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct OverallView {
    pub ev: EstimateView,
    /// 總盈虧，bb
    pub net_bb: f64,
    /// 獲勝手數比例。分母是**總手數**（含棄牌手），9-max 下典型僅
    /// 10–15%，定位為輔助指標
    pub hands_won: ProportionView,
    /// 平分手數。F.2 明訂 tie 另列，不混入 wins
    #[ts(type = "number")]
    pub ties: u64,
    pub showdown_won: ProportionView,
    /// 每手結果樣本標準差，bb
    pub sigma_bb: f64,
    /// 每百手標準差，bb
    pub sigma100_bb: f64,
    /// 累計盈虧曲線的最大峰谷差，bb
    pub max_drawdown_bb: f64,
    /// All-in EV，bb/100。
    ///
    /// 恆為 null：這個指標要的是 all-in 節點之後**分段（main／side pot）**
    /// 的 equity（核心規格 5.2），而目前的逐手紀錄只存行動與結算，沒有
    /// 分段 equity。用實際結算冒充 All-in EV 會讓它退化成另一個 EV，
    /// 失去「排除 runout 運氣」這個唯一的用途，因此寧可留空
    #[ts(type = "number | null")]
    pub all_in_ev_bb100: Option<f64>,
}

/// 逐位置表的一列（UI 規格 F.4）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PositionRowView {
    /// 當手在桌人數。9 人桌的 BTN 與 7 人桌的 BTN 是不同母體，
    /// 不得合併為同一列
    pub seated: u8,
    pub position: String,
    pub ev: EstimateView,
    pub hands_won: ProportionView,
    pub vpip: ProportionView,
    pub pfr: ProportionView,
}

/// 桌次卡（UI 規格 F.4）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct TablesView {
    #[ts(type = "number")]
    pub instance_count: u64,
    /// 平均存活手數。使用者的桌次存活手數本身是策略強弱的觀測值
    pub avg_hands_alive: f64,
    /// 結束原因分佈
    pub end_reasons: Vec<EndReasonView>,
    #[ts(type = "number")]
    pub refill_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct EndReasonView {
    /// heroBusted／notEnoughPlayers／handLimitReached
    pub reason: String,
    #[ts(type = "number")]
    pub count: u64,
}

/// 面板 F 的完整報表。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ReportView {
    pub scope: ReportScopeView,
    pub overall: OverallView,
    /// 行為頻率：VPIP／PFR／3-bet／C-bet／Fold to C-bet／WTSD／W$SD
    pub frequencies: Vec<ProportionView>,
    pub positions: Vec<PositionRowView>,
    pub tables: TablesView,
    /// 逐位置表是否納入 dead button／dead small blind 手
    pub include_dead: bool,
    /// 被逐位置表排除的手數。整體卡**不**排除這些手
    #[ts(type = "number")]
    pub dead_hands: u64,
}

// ── 逐手事實 ─────────────────────────────────────────────────────────

/// 一手牌裡與使用者統計有關的全部事實。
///
/// 每一個布林都在讀紀錄時就定案，聚合階段只做加總——「這手算不算
/// C-bet」的判斷散落在聚合迴圈裡的話，改一個定義要改好幾處。
struct HandFacts {
    instance: u64,
    seated: u8,
    position: Option<PositionLabel>,
    /// dead button 或 dead small blind。這些手少收一份強制下注，
    /// 底池結構與一般手不同，預設排除於逐位置切片
    dead: bool,
    delta_bb: f64,
    vpip: bool,
    pfr: bool,
    /// 翻前面對加注的次數，即 3-bet 的分母
    faced_raise: u64,
    three_bet: u64,
    cbet_chance: bool,
    cbet: bool,
    faced_cbet: bool,
    fold_to_cbet: bool,
    saw_flop: bool,
    showdown: bool,
    /// 攤牌且獨贏
    showdown_sole_win: bool,
    /// 攤牌且贏得籌碼（含平分）
    showdown_money: bool,
    sole_win: bool,
    tie: bool,
}

/// 從一手紀錄取出使用者的統計事實。
///
/// 使用者未入座（該手不在桌）時回傳 `None`——那手根本不是他的樣本。
///
/// 下注門檻由 `committed_to` 重建，與重播幀（[`crate::frames`]）同一套
/// 規則：ante 是 dead money 不進當街投入，盲注與 straddle 進。
/// 「這個行動是不是進攻」一律以 `committed_to > 當下門檻` 判定，
/// 而不是看行動種類——`AllIn` 可能是加注也可能是被迫全下的跟注，
/// 只看種類會把後者算成 PFR。
#[allow(clippy::cast_precision_loss)]
fn analyse(record: &HandRecord, hero: usize, hero_delta: i64, big_blind: u64) -> Option<HandFacts> {
    if !record.occupied.get(hero).copied().unwrap_or(false) {
        return None;
    }
    let seats = record.occupied.len();
    let positions = resolve(&record.occupied, usize::from(record.big_blind_seat));

    // 翻前的開牌門檻：盲注與 straddle 的最高額。高於它才叫「加注」，
    // 否則 BB 位的盲注會被當成一次加注，所有人都在「面對加注」
    let open_level = record
        .posts
        .iter()
        .filter(|p| !matches!(p.kind, PostKind::Ante))
        .map(|p| p.amount.units())
        .max()
        .unwrap_or(0);

    let mut level = open_level;
    let mut street = Street::Preflop;
    let mut folded = vec![false; seats];

    let mut vpip = false;
    let mut pfr = false;
    let mut faced_raise = 0u64;
    let mut three_bet = 0u64;
    let mut preflop_aggressor: Option<usize> = None;
    let mut hero_folded_preflop = false;

    let mut cbet_chance = false;
    let mut cbet = false;
    let mut faced_cbet = false;
    let mut fold_to_cbet = false;
    // 翻牌圈的第一個下注由誰打出。是翻前攻擊者打的才叫 C-bet
    let mut first_flop_bet: Option<usize> = None;
    // 使用者在翻牌圈的 C-bet 機會／面對 C-bet 只認第一次，
    // 同一手裡的後續下注輪不是 C-bet
    let mut flop_settled = false;

    for action in &record.actions {
        if action.street != street {
            street = action.street;
            level = 0;
        }
        let seat = usize::from(action.seat);
        let before = level;
        // 進攻 = 把門檻推高。RaiseTo 恆成立；AllIn 只有超過門檻才算
        let aggressive = action.committed_to.units() > before;

        if seat == hero {
            match street {
                Street::Preflop => {
                    let faced = before > open_level;
                    if faced {
                        faced_raise += 1;
                    }
                    if matches!(
                        action.action,
                        Action::Call | Action::RaiseTo(_) | Action::AllIn
                    ) {
                        vpip = true;
                    }
                    if aggressive {
                        pfr = true;
                        if faced {
                            three_bet += 1;
                        }
                    }
                    if matches!(action.action, Action::Fold) {
                        hero_folded_preflop = true;
                    }
                }
                Street::Flop if !flop_settled => {
                    if preflop_aggressor == Some(hero) && before == 0 {
                        cbet_chance = true;
                        cbet = aggressive;
                        flop_settled = true;
                    } else if first_flop_bet.is_some() && first_flop_bet == preflop_aggressor {
                        faced_cbet = true;
                        fold_to_cbet = matches!(action.action, Action::Fold);
                        flop_settled = true;
                    }
                }
                _ => {}
            }
        }

        if aggressive {
            level = action.committed_to.units();
            match street {
                Street::Preflop => preflop_aggressor = Some(seat),
                Street::Flop if before == 0 && first_flop_bet.is_none() => {
                    first_flop_bet = Some(seat);
                }
                _ => {}
            }
        }
        if matches!(action.action, Action::Fold) {
            folded[seat] = true;
        }
    }

    // 攤牌 = 收池時還有兩個以上沒棄牌的人。只剩一個人的手是全部棄牌，
    // 沒有攤牌，即使贏家亮牌也一樣
    let contenders = (0..seats)
        .filter(|&s| record.occupied[s] && !folded[s])
        .count();
    let showdown = contenders >= 2 && !folded[hero];
    let hero_payout = record.payouts.get(hero).map_or(0, |c| c.units());
    // 獨贏 = 只有使用者拿到 payout。分池時短碼贏 main pot、使用者贏
    // side pot 的情形不算獨贏——那手他確實沒有把整個底池拿走
    let sole_win = hero_payout > 0
        && (0..seats).all(|s| s == hero || record.payouts.get(s).is_none_or(|c| c.units() == 0));

    Some(HandFacts {
        instance: record.instance_index,
        seated: u8::try_from(record.occupied.iter().filter(|&&o| o).count()).unwrap_or(u8::MAX),
        position: positions.labels.get(hero).and_then(|l| *l),
        dead: positions.dead_button || positions.dead_small_blind,
        delta_bb: if big_blind == 0 {
            0.0
        } else {
            hero_delta as f64 / big_blind as f64
        },
        vpip,
        pfr,
        faced_raise,
        three_bet,
        cbet_chance,
        cbet,
        faced_cbet,
        fold_to_cbet,
        saw_flop: !hero_folded_preflop && record.board.len() >= 3,
        showdown,
        showdown_sole_win: showdown && sole_win,
        showdown_money: showdown && hero_payout > 0,
        sole_win,
        tie: hero_payout > 0 && !sole_win,
    })
}

// ── 聚合 ─────────────────────────────────────────────────────────────

/// 一組手牌的 bb/100 估計。
///
/// `seed` 讓同一個 run 的報表每次都算出同一個區間：bootstrap 會抽樣，
/// 沒有固定種子的話重新開啟面板就會看到不一樣的 CI，使用者會合理地
/// 認為其中一次是錯的。
fn estimate(facts: &[&HandFacts], seed: u64, resamples: usize) -> Estimate {
    let observations: Vec<Observation> = facts
        .iter()
        .map(|f| Observation {
            delta_bb: f.delta_bb,
            instance: f.instance,
        })
        .collect();
    let series: Vec<f64> = observations.iter().map(|o| o.delta_bb).collect();
    let block = estimate_block_length(&series);
    let mut rng = Rng::derive(seed, 0, RngDomain::Stats);
    moving_block_bootstrap(&observations, block, resamples, &mut rng)
}

/// 樣本標準差、每百手標準差與最大回撤。
#[allow(clippy::cast_precision_loss)]
fn dispersion(facts: &[&HandFacts]) -> (f64, f64, f64) {
    let n = facts.len();
    if n < 2 {
        return (0.0, 0.0, 0.0);
    }
    let mean = facts.iter().map(|f| f.delta_bb).sum::<f64>() / n as f64;
    let variance = facts
        .iter()
        .map(|f| (f.delta_bb - mean).powi(2))
        .sum::<f64>()
        / (n - 1) as f64;
    let sigma = variance.sqrt();

    // 最大回撤：累計曲線的最大峰谷差。用累計而非逐手，因為連續小輸
    // 造成的深谷才是使用者真正會經歷的事
    let mut cumulative = 0.0;
    let mut peak = 0.0f64;
    let mut drawdown = 0.0f64;
    for f in facts {
        cumulative += f.delta_bb;
        peak = peak.max(cumulative);
        drawdown = drawdown.max(peak - cumulative);
    }

    (sigma, sigma * 10.0, drawdown)
}

fn count(facts: &[&HandFacts], predicate: impl Fn(&HandFacts) -> bool) -> u64 {
    facts.iter().filter(|f| predicate(f)).count() as u64
}

/// 行為頻率（UI 規格 F.4／F.2）。
fn frequencies(facts: &[&HandFacts]) -> Vec<ProportionView> {
    let hands = facts.len() as u64;
    let faced_raise: u64 = facts.iter().map(|f| f.faced_raise).sum();
    let three_bet: u64 = facts.iter().map(|f| f.three_bet).sum();

    vec![
        ProportionView::new(
            "VPIP",
            "自願投入籌碼進池手數 ÷ 總手數",
            wilson(count(facts, |f| f.vpip), hands),
        ),
        ProportionView::new(
            "PFR",
            "翻前加注手數 ÷ 總手數",
            wilson(count(facts, |f| f.pfr), hands),
        ),
        ProportionView::new(
            "3-bet",
            "翻前再加注次數 ÷ 面對加注機會次數",
            wilson(three_bet, faced_raise),
        ),
        ProportionView::new(
            "C-bet",
            "翻牌圈下注次數 ÷ 作為翻前攻擊者可下注次數",
            wilson(count(facts, |f| f.cbet), count(facts, |f| f.cbet_chance)),
        ),
        ProportionView::new(
            "Fold to C-bet",
            "面對 C-bet 棄牌次數 ÷ 面對 C-bet 次數",
            wilson(
                count(facts, |f| f.fold_to_cbet),
                count(facts, |f| f.faced_cbet),
            ),
        ),
        ProportionView::new(
            "WTSD",
            "進入攤牌手數 ÷ 看到翻牌手數",
            wilson(
                count(facts, |f| f.showdown && f.saw_flop),
                count(facts, |f| f.saw_flop),
            ),
        ),
        ProportionView::new(
            "W$SD",
            "攤牌贏得籌碼手數 ÷ 進入攤牌手數（含平分，與「攤牌獲勝比例」的獨贏定義不同）",
            wilson(
                count(facts, |f| f.showdown_money),
                count(facts, |f| f.showdown),
            ),
        ),
    ]
}

/// 逐位置表。
///
/// F.4：**必須依當手在桌人數切片**。使用者在 9 人桌的 BTN 與 7 人桌的
/// BTN 是不同母體，合併成一列會把兩種完全不同的處境平均掉。
fn position_rows(facts: &[&HandFacts], seed: u64) -> Vec<PositionRowView> {
    let mut groups: BTreeMap<(u8, PositionLabel), Vec<&HandFacts>> = BTreeMap::new();
    for f in facts {
        if let Some(label) = f.position {
            groups.entry((f.seated, label)).or_default().push(f);
        }
    }

    groups
        .into_iter()
        .map(|((seated, label), rows)| {
            let hands = rows.len() as u64;
            PositionRowView {
                seated,
                position: label.as_str().to_owned(),
                ev: estimate(&rows, seed, SLICE_RESAMPLES).into(),
                hands_won: ProportionView::new(
                    "獲勝手數比例",
                    "獨贏手數 ÷ 該切片手數",
                    wilson(count(&rows, |f| f.sole_win), hands),
                ),
                vpip: ProportionView::new(
                    "VPIP",
                    "自願投入籌碼進池手數 ÷ 該切片手數",
                    wilson(count(&rows, |f| f.vpip), hands),
                ),
                pfr: ProportionView::new(
                    "PFR",
                    "翻前加注手數 ÷ 該切片手數",
                    wilson(count(&rows, |f| f.pfr), hands),
                ),
            }
        })
        .collect()
}

/// 桌次卡。
#[allow(clippy::cast_precision_loss)]
fn tables(manifest: &poker_storage::manifest::RunManifest) -> TablesView {
    let instances = &manifest.instances;
    let total_hands: u64 = instances.iter().map(|i| i.hands).sum();
    let mut reasons: BTreeMap<&str, u64> = BTreeMap::new();
    for instance in instances {
        *reasons.entry(instance.end.as_str()).or_default() += 1;
    }

    TablesView {
        instance_count: instances.len() as u64,
        avg_hands_alive: if instances.is_empty() {
            0.0
        } else {
            total_hands as f64 / instances.len() as f64
        },
        end_reasons: reasons
            .into_iter()
            .map(|(reason, count)| EndReasonView {
                reason: reason.to_owned(),
                count,
            })
            .collect(),
        refill_count: instances.iter().map(|i| i.refills.len() as u64).sum(),
    }
}

/// 組出面板 F 的報表。
///
/// `include_dead` 只作用於逐位置切片；整體卡的 EV／bb100 **不排除**
/// dead 手（UI 規格 F.4）——那些手照樣是使用者打過的手，從總體結論裡
/// 拿掉會讓總盈虧與逐手 log 對不起來。
///
/// # Errors
/// run 不存在、讀取或解碼失敗時回傳錯誤。
pub fn report(store: &Store, run_id: i64, include_dead: bool) -> Result<ReportView, IpcError> {
    let manifest = store.load_manifest(run_id)?;
    let hero = manifest.hero_seat;
    let big_blind = manifest.big_blind;

    let mut facts: Vec<HandFacts> = Vec::new();
    let mut offset = 0u64;
    loop {
        let page = store.page_hand_summaries(run_id, offset, PAGE)?;
        if page.is_empty() {
            break;
        }
        offset += page.len() as u64;
        for (record, hero_delta) in &page {
            if let Some(f) = analyse(record, hero, *hero_delta, big_blind) {
                facts.push(f);
            }
        }
    }

    let all: Vec<&HandFacts> = facts.iter().collect();
    let sliced: Vec<&HandFacts> = if include_dead {
        all.clone()
    } else {
        all.iter().copied().filter(|f| !f.dead).collect()
    };

    let hands = all.len() as u64;
    let (sigma_bb, sigma100_bb, max_drawdown_bb) = dispersion(&all);
    let seed = manifest.master_seed;

    let overall = OverallView {
        ev: estimate(&all, seed, DEFAULT_RESAMPLES).into(),
        net_bb: all.iter().map(|f| f.delta_bb).sum(),
        hands_won: ProportionView::new(
            "獲勝手數比例",
            "獨贏手數 ÷ 總手數（含棄牌手，9-max 下典型 10–15%）",
            wilson(count(&all, |f| f.sole_win), hands),
        ),
        ties: count(&all, |f| f.tie),
        showdown_won: ProportionView::new(
            "攤牌獲勝比例",
            "攤牌獨贏手數 ÷ 進入攤牌手數",
            wilson(
                count(&all, |f| f.showdown_sole_win),
                count(&all, |f| f.showdown),
            ),
        ),
        sigma_bb,
        sigma100_bb,
        max_drawdown_bb,
        all_in_ev_bb100: None,
    };

    Ok(ReportView {
        scope: ReportScopeView {
            run_id,
            label: "本次 run".to_owned(),
            hands,
            instance_count: manifest.instances.len() as u64,
            players: u8::try_from(manifest.players).unwrap_or(u8::MAX),
            hero_seat: u8::try_from(hero).unwrap_or(u8::MAX),
            big_blind,
            engine_version: manifest.engine_version.clone(),
            baseline_version: manifest.baseline_version.clone(),
            hero_strategy: format!(
                "{} v{}",
                manifest.hero_strategy.name, manifest.hero_strategy.version
            ),
            bot_personas: manifest
                .bot_personas
                .iter()
                .map(|p| p.name.clone())
                .collect(),
            created_at: manifest.created_at,
            completed: manifest.completed,
        },
        overall,
        frequencies: frequencies(&all),
        positions: position_rows(&sliced, seed),
        tables: tables(&manifest),
        include_dead,
        dead_hands: count(&all, |f| f.dead),
    })
}
