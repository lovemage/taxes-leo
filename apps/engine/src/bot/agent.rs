//! 把 [`DecisionView`] 接到七步決策管線。
//!
//! 在這之前，執行層對全部座位用的是「能過牌就過牌，不然就跟」的佔位
//! 策略，所以人格與行為參數調了也不會改變任何結果。這個模組是缺的
//! 那一段接線：節點識別 → 基準分佈 → [`pipeline::run`] → 行動。
//!
//! 翻前走 [`BaselineRules`]；翻後先走具版本的 equity heuristic baseline。
//! 後者依英雄可見牌、公共牌、存活對手數與底池賠率產生分佈，不讀取任何
//! 隱藏資訊。它是可實際下注的工程基準，不冒充尚未交付的顧問規則表。

use std::collections::BTreeMap;

use crate::betting::{Action, LegalActions};
use crate::bot::params::ParamValue;
use crate::bot::pipeline::{self, BotConfig, PipelineError};
use crate::chips::Chips;
use crate::equity::monte_carlo_vs_random;
use crate::hand::{ActionProvider, Street};
use crate::position::PositionLabel;
use crate::rng::{Rng, RngDomain};
use crate::strategy::baseline::{self, BaselineRules};
use crate::strategy::default_chart::ChartShift;
use crate::strategy::cell_override::CellOverrides;
use crate::strategy::distribution::{ActionDistribution, Myriad, FULL};
use crate::strategy::preflop::{PreflopNode, PreflopScenario};
use crate::strategy::ranking::EquityRanking;
use crate::strategy::DecisionView;

/// 翻後工程基準的版本字串。
pub const POSTFLOP_BASELINE_VERSION: &str = "equityHeuristic/v1-unapproved";

/// 工程基準也無法產生合法分佈時使用的最後 fallback。
pub const POSTFLOP_FALLBACK_VERSION: &str = "checkFold/v0";

/// 每個翻後決策的批次 equity 樣本數。
///
/// 這是工程 heuristic 的固定內容之一，因此版本升級時必須一併檢視。
/// 現有 release benchmark 即使 8 名對手也低於每決策 1 ms 的預算。
pub const POSTFLOP_EQUITY_SAMPLES: u64 = 64;

/// `baseline::expected_opponents` 可能回傳的最大值。
///
/// 上界來自 `VsLimp` 的 `limpers.clamp(1, 3) + 1`。這個常數與那個算式
/// 綁在一起，由 `rankings_cover_expected_opponents` 測試守住。
pub const MAX_EXPECTED_OPPONENTS: usize = 4;

/// 依 `DecisionView` 決策的 Bot。
///
/// 逐座持有一份 [`BotConfig`]，因此不同座位可以是不同人格。
pub struct BotAgent {
    /// 逐座的**未套用人格**規則集。
    ///
    /// 管線第 5 步夾的是「偏離基準的幅度」，而人格已經在內容層動過手
    /// （見 `ChartShift`）。沒有這一份，進管線的分佈自己就是基準，
    /// 上限永遠量到 0
    reference: Vec<BaselineRules>,
    /// 逐座規則集，已套用該座的 `rangeWidth`。
    ///
    /// 在建構時就縮放好，而不是每次決策才算：縮放要走遍全部寬度表，
    /// 每個決策做一次會是純粹的浪費
    rules: Vec<BaselineRules>,
    /// 依「預期對手數」分開的 equity 排序。
    ///
    /// 開牌時桌上還有很多人會蓋牌，實際攤牌對手遠少於在座人數；
    /// 用同一份排序處理所有情境，會出現「UTG 開 K9s 卻蓋 88」這種
    /// 排序假象（見 `baseline::expected_opponents`）
    rankings: BTreeMap<usize, EquityRanking>,
    /// 逐座設定。索引即座位序
    seats: Vec<BotConfig>,
    master_seed: u64,
    /// 決策序號，供 RNG stream 派生。
    ///
    /// 用序號而非手序，是因為 [`ActionProvider`] 拿不到手序。同一組設定
    /// 與 seed 會產生同一串決策，因此序號同樣可重現；暫停續跑也不影響
    decisions: u64,
}

/// 由 Bot 設定推導出該座位實際生效的基準規則。
///
/// [`BotAgent::new`] 與面板 C 的範圍預覽共用這一支。各寫一份的話，預覽
/// 畫出來的就不是 Bot 實際會打的東西——而「看出 Bot 會怎麼打」正是那個
/// 預覽存在的唯一理由，畫錯比不畫更糟。
#[must_use]
pub fn rules_for_bot(rules: &BaselineRules, config: &BotConfig) -> BaselineRules {
    let myriad = |key: &str| {
        config
            .effective(key)
            .and_then(ParamValue::as_myriad)
            .and_then(|v| Myriad::try_from(v).ok())
            .unwrap_or(FULL)
    };
    let mut scaled = rules.scaled(myriad("rangeWidth"));
    // 預設組合表是純策略，管線的權重縮放對它是空操作；這三個人格參數
    // 必須在內容層作用，否則走表的節點上三支滑桿全是裝飾品（見 `ChartShift`）
    scaled.chart_shift = ChartShift {
        range_width: myriad("rangeWidth"),
        aggression: myriad("preflopAggression"),
        call_persistence: myriad("callPersistence"),
        fold_discipline: myriad("foldDiscipline"),
    };
    scaled
}

impl BotAgent {
    #[must_use]
    pub fn new(
        rules: BaselineRules,
        rankings: BTreeMap<usize, EquityRanking>,
        seats: Vec<BotConfig>,
        master_seed: u64,
    ) -> Self {
        let reference = rules.clone();
        let rules = seats.iter().map(|config| rules_for_bot(&rules, config)).collect();
        Self {
            reference: vec![reference; seats.len()],
            rules,
            rankings,
            seats,
            master_seed,
            decisions: 0,
        }
    }

    /// 以 `samples` 次取樣建立所需的 equity 排序。
    ///
    /// 涵蓋 1–4 名對手，因為 `baseline::expected_opponents` 在多人跛入的
    /// 情境會要到 4（`limpers.clamp(1, 3) + 1`）。少算的話那些節點會退回
    /// 單挑排序，兩名以上跛入者的範圍就系統性偏緊——單挑該打的牌與
    /// 五人底池該打的牌差很多。
    #[must_use]
    pub fn rankings(samples: u64) -> BTreeMap<usize, EquityRanking> {
        (1..=MAX_EXPECTED_OPPONENTS)
            .map(|opponents| (opponents, EquityRanking::compute(opponents, samples)))
            .collect()
    }

    /// 為單一座位裝上逐格覆寫（面板 D 的自身策略）。
    ///
    /// **只裝在使用者座位上。** 裝到全桌等於偷偷改掉對手，跑出來的統計
    /// 就不是在測自己的策略了。座位超出範圍時不做事：座位數由
    /// `SessionConfig::validate` 把關，這裡不當第二個驗證點。
    pub fn set_seat_overrides(&mut self, seat: usize, overrides: CellOverrides) {
        if let Some(rules) = self.rules.get_mut(seat) {
            rules.overrides = overrides.clone();
        }
        // 基準也要跟著裝：覆寫是使用者親手訂的內容，不是人格偏移，
        // 夾幅度時不該把它算成「偏離基準」
        if let Some(rules) = self.reference.get_mut(seat) {
            rules.overrides = overrides;
        }
    }

    fn config_for(&self, seat: usize) -> &BotConfig {
        self.seats
            .get(seat)
            .unwrap_or_else(|| self.seats.first().expect("至少一組 Bot 設定"))
    }

    fn rules_for(&self, seat: usize) -> &BaselineRules {
        self.rules
            .get(seat)
            .unwrap_or_else(|| self.rules.first().expect("至少一組規則"))
    }

    fn reference_for(&self, seat: usize) -> &BaselineRules {
        self.reference
            .get(seat)
            .unwrap_or_else(|| self.reference.first().expect("至少一組規則"))
    }
}

impl ActionProvider for BotAgent {
    fn choose(&mut self, view: &DecisionView) -> Action {
        self.decisions += 1;
        let mut rng = Rng::derive(self.master_seed, self.decisions, RngDomain::StrategyMix);
        let roll = Myriad::try_from(rng.below(u64::from(FULL))).unwrap_or(0);

        let config = self.config_for(view.seat);
        let fit = |d: ActionDistribution| {
            drop_free_fold(&fit_raise_sizes(&d, &view.legal), &view.legal)
        };
        let (baseline, reference, aggression_key) = match view.street {
            Street::Preflop => (
                preflop_baseline(view, self.rules_for(view.seat), &self.rankings).map(fit),
                preflop_baseline(view, self.reference_for(view.seat), &self.rankings).map(fit),
                "preflopAggression",
            ),
            _ => {
                // Equity 與策略混頻使用不同 RNG domain；調整 equity 樣本數不會
                // 改變相同節點用來抽 action distribution 的亂數。
                let mut equity_rng =
                    Rng::derive(self.master_seed, self.decisions, RngDomain::Equity);
                let baseline = postflop_baseline(view, &mut equity_rng).map(fit);
                (baseline.clone(), baseline, "postflopAggression")
            }
        };

        let Some(baseline) = baseline else {
            return fallback(&view.legal);
        };
        let reference = reference.unwrap_or_else(|| baseline.clone());

        match pipeline::run_with_reference_and_aggression(
            &reference,
            &baseline,
            config,
            aggression_key,
            legality(&view.legal),
            roll,
        ) {
            Ok(trace) => trace.final_action,
            // 遮蔽後無合法行動：核心規格 4.2 要求進入 fallback，
            // 不得除以 0 或任選行動
            Err(PipelineError::NoLegalAction | PipelineError::Distribution(_)) => {
                fallback(&view.legal)
            }
        }
    }
}

/// 翻後工程基準。
///
/// 以對隨機合法手牌的 Monte Carlo equity 做粗粒度分桶，再用底池賠率決定
/// 面對下注時的跟注門檻。這不是 solver/GTO 輸出，但至少是可重現、可版本化、
/// 會依牌力與牌局狀態改變的策略，不再讓所有玩家一路 check 到攤牌。
fn postflop_baseline(view: &DecisionView, rng: &mut Rng) -> Option<ActionDistribution> {
    if !(3..=5).contains(&view.board.len()) {
        return None;
    }

    let opponents = view.active_opponents().max(1);
    let equity = u32::try_from(monte_carlo_vs_random(
        view.hole_cards,
        opponents,
        &view.board,
        POSTFLOP_EQUITY_SAMPLES,
        rng,
    )
    .as_myriad())
    .unwrap_or(FULL)
    .min(FULL);
    let opponents = u32::try_from(opponents).unwrap_or(8);
    let fair_share = FULL / opponents.saturating_add(1);
    let aggressive = Action::RaiseTo(postflop_raise_to(view, equity));

    let weights = if view.legal.can_check {
        if equity >= fair_share.saturating_add(2_500) {
            // 明顯領先：以價值下注為主，保留少量過牌設陷。
            vec![(aggressive, 8_000), (Action::Check, 2_000)]
        } else if equity >= fair_share.saturating_add(1_000) {
            vec![(aggressive, 6_000), (Action::Check, 4_000)]
        } else if equity >= fair_share.saturating_sub(300) {
            // 接近平均勝率：小比例下注、以控池為主。
            vec![(aggressive, 2_500), (Action::Check, 7_500)]
        } else {
            // 弱牌保留少量詐唬；多人底池再收緊。
            let bluff = if opponents == 1 { 800 } else { 300 };
            vec![(aggressive, bluff), (Action::Check, 10_000 - bluff)]
        }
    } else {
        let pot_odds = postflop_pot_odds(view);
        if equity >= fair_share.saturating_add(2_500)
            && equity >= pot_odds.saturating_add(1_500)
        {
            // 強牌面對下注：價值加注與跟注設陷混合。
            vec![(aggressive, 4_500), (Action::Call, 5_500)]
        } else if equity >= fair_share.saturating_add(700)
            && equity >= pot_odds.saturating_add(700)
        {
            vec![(aggressive, 1_500), (Action::Call, 7_500), (Action::Fold, 1_000)]
        } else if equity >= pot_odds.saturating_add(300) {
            vec![(Action::Call, 7_000), (Action::Fold, 3_000)]
        } else if equity.saturating_add(400) >= pot_odds {
            vec![(Action::Call, 3_500), (Action::Fold, 6_500)]
        } else {
            // 明顯低於底池賠率時大多棄牌，但不是永遠 fold 的機器。
            vec![(aggressive, 300), (Action::Call, 400), (Action::Fold, 9_300)]
        }
    };

    ActionDistribution::from_weights(weights).ok()
}

/// 依底池與 equity 選擇一個 50%／66%／75% pot 的下注意圖。
/// 最終仍由 [`fit_raise_sizes`] 夾進引擎提供的合法區間。
fn postflop_raise_to(view: &DecisionView, equity: Myriad) -> Chips {
    let own_committed = view
        .history
        .iter()
        .rev()
        .find(|action| action.street == view.street && action.seat == view.seat)
        .map_or(Chips::ZERO, |action| action.committed_to);
    let base = view.legal.call_to.unwrap_or(own_committed);
    let pot_after_call = view.pot + view.to_call;
    let percent = if equity >= 7_500 {
        75
    } else if equity >= 5_000 || view.street == Street::River {
        66
    } else {
        50
    };
    Chips::new(
        base.units()
            .saturating_add(pot_after_call.units().saturating_mul(percent) / 100),
    )
}

/// 跟注所需 equity（萬分比）：call ÷（目前底池 + call）。
fn postflop_pot_odds(view: &DecisionView) -> Myriad {
    let call = view.to_call.units();
    let final_pot = view.pot.units().saturating_add(call);
    if call == 0 || final_pot == 0 {
        return 0;
    }
    u32::try_from(call.saturating_mul(u64::from(FULL)) / final_pot)
        .unwrap_or(FULL)
        .min(FULL)
}

/// 翻前基準分佈。
fn preflop_baseline(
    view: &DecisionView,
    rules: &BaselineRules,
    rankings: &BTreeMap<usize, EquityRanking>,
) -> Option<ActionDistribution> {
    let node = PreflopNode {
        seated: u8::try_from(view.seated).ok()?,
        hero: view.position,
        bucket: view.effective_stack_bucket,
        scenario: scenario_of(view),
    };
    // 取不到對應的排序寧可放棄這個節點走 fallback，也不退回別的人數。
    // 退回單挑排序會讓多人底池的範圍看起來正常、實際上系統性錯誤——
    // 那種錯誤不會報錯，只會靜靜地污染統計
    let ranking = rankings.get(&baseline::expected_opponents(&node))?;
    baseline::distribution_for(&node, view.hand_class(), rules, ranking, view.big_blind).ok()
}

/// 由公開行動歷史識別翻前情境（核心規格 4.1 的 node 要素）。
///
/// 只看**本手翻前**的公開行動；底牌與牌堆順序不參與，因此這個函式
/// 拿不到也用不到隱藏資訊。
#[must_use]
pub fn scenario_of(view: &DecisionView) -> PreflopScenario {
    let mut raisers: Vec<PositionLabel> = Vec::new();
    let mut limpers = 0u8;
    let mut hero_raise_index: Option<usize> = None;
    // 英雄加注之後、下一個加注之前，有沒有人跟注——squeeze 與 3-bet 的差別
    let mut callers_since_hero_raise = 0u8;

    for action in view.history.iter().filter(|a| a.street == Street::Preflop) {
        match action.action {
            // 是不是加注看引擎記的 `raised`，不看行動種類。
            // 籌碼不足的全下只是部分跟注，把它算成一次加注會讓後手把
            // 「開牌 ＋ 短碼全下跟」誤判成 3-bet，套用完全不同的範圍
            Action::RaiseTo(_) | Action::AllIn if action.raised => {
                if action.seat == view.seat {
                    hero_raise_index = Some(raisers.len());
                    callers_since_hero_raise = 0;
                }
                raisers.push(action.position);
            }
            // 沒推高注額的全下等同跟注
            Action::Call | Action::AllIn | Action::RaiseTo(_) => {
                if raisers.is_empty() {
                    limpers = limpers.saturating_add(1);
                } else if hero_raise_index == Some(raisers.len() - 1) {
                    callers_since_hero_raise = callers_since_hero_raise.saturating_add(1);
                }
            }
            Action::Fold | Action::Check => {}
        }
    }

    match raisers.len() {
        0 => {
            if limpers == 0 {
                PreflopScenario::Unopened
            } else {
                PreflopScenario::VsLimp { limpers }
            }
        }
        1 => PreflopScenario::VsOpen {
            opener: raisers[0],
        },
        2 => {
            let by = raisers[1];
            // 英雄自己沒下過注 → 前方是「開牌＋再加注」，這是冷 4-bet 的
            // 決策，賠率與範圍都與「自己開牌後被 3-bet」不同，不得併用
            if hero_raise_index.is_none() {
                PreflopScenario::VsOpenRaise { opener: raisers[0] }
            } else if hero_raise_index == Some(0) && callers_since_hero_raise > 0 {
                // 英雄開牌、有人跟注後被再加注才是 squeeze；
                // 中間沒有跟注者的話那只是單純的 3-bet
                PreflopScenario::VsSqueeze { by }
            } else {
                PreflopScenario::VsThreeBet { by }
            }
        }
        _ => PreflopScenario::VsFourBet {
            by: *raisers.last().expect("已知非空"),
        },
    }
}

/// 把基準分佈裡的加注尺度夾進引擎給的合法區間。
///
/// 內容表寫的是「加注到 3BB」這種**意圖**；面對 3-bet 時 3BB 可能低於
/// 最小加注額。不夾的話那份權重會在 legal mask 被整段清掉，Bot 就變成
/// 永遠不 3-bet——那不是策略，是接線失誤。
///
/// 合法性本身仍然只由引擎判定（核心規格 2.2），這裡只是把意圖投影到
/// 引擎給的區間上。
fn fit_raise_sizes(distribution: &ActionDistribution, legal: &LegalActions) -> ActionDistribution {
    let Some(range) = legal.raise else {
        return distribution.clone();
    };
    let entries: Vec<(Action, Myriad)> = distribution
        .entries()
        .iter()
        .map(|&(action, weight)| match action {
            Action::RaiseTo(to) => (
                Action::RaiseTo(to.clamp(range.min_to, range.max_to)),
                weight,
            ),
            other => (other, weight),
        })
        .collect();
    ActionDistribution::new(entries).unwrap_or_else(|_| distribution.clone())
}

/// 可以免費過牌時移除棄牌權重。
///
/// 棄牌在規則上永遠合法，因此 legal mask 不會擋它。但沒有人要你付錢的
/// 時候棄牌是**嚴格劣勢**——放棄一個免費的翻牌，換不到任何東西。
/// 內容表寫的是「這手不值得投錢」，不是「這手連免費的牌都不要看」。
///
/// 這不是策略內容，是把內容表的意圖投影到「不必付錢」這個情境上。
fn drop_free_fold(distribution: &ActionDistribution, legal: &LegalActions) -> ActionDistribution {
    if !legal.can_check {
        return distribution.clone();
    }
    let entries: Vec<(Action, Myriad)> = distribution
        .entries()
        .iter()
        .map(|&(action, weight)| match action {
            // 權重整份移到過牌，而不是丟掉。丟掉會讓其餘行動的相對比例
            // 改變，等於偷偷把 Bot 變得更愛加注
            Action::Fold => (Action::Check, weight),
            other => (other, weight),
        })
        .collect();
    ActionDistribution::from_weights(
        entries
            .into_iter()
            .fold(Vec::new(), |mut acc: Vec<(Action, u64)>, (action, weight)| {
                // 原本就有 Check 的話要合併，否則分佈會有重複鍵
                if let Some(slot) = acc.iter_mut().find(|(a, _)| *a == action) {
                    slot.1 += u64::from(weight);
                } else {
                    acc.push((action, u64::from(weight)));
                }
                acc
            }),
    )
    .unwrap_or_else(|_| distribution.clone())
}

/// 合法性判定。一律問引擎，策略層不自行推導（核心規格 2.2）。
fn legality(legal: &LegalActions) -> impl Fn(Action) -> bool + '_ {
    move |action| match action {
        Action::Fold => legal.can_fold,
        Action::Check => legal.can_check,
        Action::Call => legal.call_to.is_some(),
        Action::AllIn => legal.all_in_to.is_some(),
        Action::RaiseTo(to) => legal
            .raise
            .is_some_and(|range| to >= range.min_to && to <= range.max_to),
    }
}

/// 策略產生失敗或 legal mask 後無行動時的最後保底。
fn fallback(legal: &LegalActions) -> Action {
    if legal.can_check {
        Action::Check
    } else if legal.can_fold {
        Action::Fold
    } else if legal.call_to.is_some() {
        Action::Call
    } else {
        Action::AllIn
    }
}

/// 供上層取得 fallback 標記，寫入 `RunManifest`。
#[must_use]
pub fn fallback_note() -> (&'static str, &'static str) {
    (
        POSTFLOP_FALLBACK_VERSION,
        "僅在策略產生失敗時使用 check／fold 保底",
    )
}
