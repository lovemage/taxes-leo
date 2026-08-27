//! 面板 D — 自身策略的檢視與逐格覆寫。
//!
//! # 為什麼這一層只有翻前
//!
//! 翻前走的是 [`BaselineRules`] 的參數化產生器，因此桌型、位置、有效
//! 籌碼 bucket 與情境都真的會改變分佈，攤開來給使用者看是有內容的。
//! **翻後沒有內容**——顧問的規則表還沒進來，一律走 fallback。因此本
//! 模組不提供翻後規則清單（UI 規格 D.5）：畫一個空的規則編輯器只會
//! 讓人以為那裡有策略。
//!
//! # 編輯路徑是逐格覆寫
//!
//! 引擎目前唯一能承載「使用者自己的決定」的結構是
//! [`CellOverrides`]，它會在 `distribution_for` 的最後一步蓋掉參數
//! 產生的結果。因此面板 D 的編輯就落在這裡：改一格就是記一筆覆寫，
//! 節點鍵完全比照引擎決策時查表用的那一組欄位，不另外定義一套。
//!
//! 覆寫只裝在**使用者座位**上（見 `run::execute`）。裝到全桌等於偷偷
//! 改掉對手，跑出來的統計就不是在測自己的策略了。

use poker_engine::position::PositionLabel;
use poker_engine::strategy::baseline::{self, BaselineRules};
use poker_engine::strategy::calibration::RangeMatrix;
use poker_engine::strategy::cell_override::{CellOverrides, OverrideCell};
use poker_engine::strategy::default_chart::{
    raise_size_centi_bb, ChartAction, ChartDepth, ChartEntry, DefaultChart, CHART_VERSION,
};
use poker_engine::strategy::decision::StackBucket;
use poker_engine::strategy::hand_class::HandClass;
use poker_engine::strategy::preflop::{
    all_buckets, enumerate_nodes, positions_for, scenarios_for, PreflopNode, PreflopScenario,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 策略內容的來源與現況（UI 規格 D.2、D.7、D.8）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct StrategyMetaView {
    pub baseline_name: String,
    pub baseline_version: String,
    /// 是否已由牌手顧問簽核。false 代表這是工程佔位內容，
    /// 不得當成校準過的結果解讀
    pub consultant_approved: bool,
    /// 低於此 bucket 一律推入或棄牌
    pub push_fold_below: String,
    /// 各情境的加注尺度，以 BB 的百分之一表示（250 = 2.5BB）
    pub open_size_centi_bb: u32,
    pub three_bet_size_centi_bb: u32,
    pub four_bet_size_centi_bb: u32,
    /// 翻後 fallback 的版本字串（核心規格 4.2 要求明示）
    pub postflop_fallback: String,
    /// 翻前節點總數（桌型 × 位置 × bucket × 情境）
    #[ts(type = "number")]
    pub preflop_node_count: u64,
    /// 翻前總格數 = 節點數 × 169
    #[ts(type = "number")]
    pub preflop_cell_count: u64,
    /// equity 排序的取樣數。面板與執行層共用同一份，兩邊看到的必須一致
    #[ts(type = "number")]
    pub ranking_samples: u64,
    /// 排序來源：`asset/v1`（離線資產）／`debugFallback`／`unavailable`
    pub ranking_source: String,
    /// 排序的取樣數是否足以產製正式內容。
    ///
    /// false 代表面板上畫的範圍**不是正式內容**（debug 低樣本替代品，
    /// 或內容根本沒載進來）。UI 必須明講——低樣本排序看起來與正式的
    /// 一模一樣，使用者沒有任何辦法自己分辨
    pub ranking_content_grade: bool,
    /// 給使用者看的一句話說明排序來源
    pub ranking_note: String,

    /// 預設組合表的來源檔名（顧問給的那份 Excel）
    pub chart_source: String,
    /// 預設組合表的內容版本
    pub chart_version: String,
    /// 表是否成功載入。false 時 `chart_note` 帶著失敗原因
    pub chart_loaded: bool,
    /// 表上的格數（深度 × 位置 × 情境）
    #[ts(type = "number")]
    pub chart_cell_count: u64,
    /// 引擎的翻前節點之中，由表提供內容的比例（萬分比）
    pub chart_coverage_myriad: u32,
    /// 給使用者看的一句話說明：表涵蓋什麼、沒涵蓋什麼
    pub chart_note: String,
}

/// 一個有效籌碼分檔（規則細則 8.5）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BucketOptionView {
    pub key: String,
    pub label: String,
    /// 這一檔採推入或棄牌，不做小額加注
    pub push_fold: bool,
    /// 該檔對範圍寬度的乘數（萬分比，10000 = 不變）。
    ///
    /// 這是**參數產生器**的欄位。走預設組合表的節點不看它——表的內容
    /// 直接就是那一檔的手牌清單
    pub multiplier: u32,
    /// 這一檔對應到預設組合表的哪一欄深度。
    ///
    /// 引擎分九檔、表只有四檔，對應關係必須說出來：否則使用者選了
    /// 40–70BB，看到的其實是表上 35–50BB 那一欄，而畫面上毫無跡象
    pub chart_depth: String,
}

/// 一個翻前情境（UI 規格 D.4 的節點情境清單）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ScenarioOptionView {
    /// 內容表的欄位鍵，與引擎決策時查表用的字串同一份
    pub key: String,
    pub label: String,
    /// 導航樹的分組標題
    pub group: String,
}

/// 某（桌型 × 位置）下的導航選項。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct StrategyNodesView {
    pub seated: u8,
    /// 該桌型由早到晚的位置序列（規則細則 8.4.1）
    pub positions: Vec<String>,
    /// 實際採用的英雄位置。呼叫端給的位置不屬於該桌型時會落到第一個
    pub hero: String,
    pub scenarios: Vec<ScenarioOptionView>,
    pub buckets: Vec<BucketOptionView>,
}

/// 13×13 矩陣中的一格。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct MatrixCellView {
    /// `AA`／`AKs`／`AKo`
    pub class: String,
    pub row: u8,
    pub col: u8,
    /// 這一類的 combo 數（對子 6、同花 4、非同花 12）
    pub combos: u16,
    /// 主動（加注或推入）頻率，萬分比
    pub aggressive: u32,
    pub call: u32,
    /// 過牌頻率。**不併進棄牌**：無人加注時大盲過牌看翻牌，
    /// 與把牌丟掉是完全不同的一件事
    pub check: u32,
    pub fold: u32,
    /// equity 排序百分位，萬分比（0 為最強）
    pub percentile: u32,
    /// 這一格是使用者覆寫的結果，不是參數產生的
    pub overridden: bool,
    /// 這一格在預設組合表上的動作鍵（`fold`／`call`／`raise-2.5x`／
    /// `raise-8x`／`allin`）。`null` 代表這個節點不在表上，走參數產生器
    pub chart_action: Option<String>,
}

/// 預設組合表上的一列（一個動作）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ChartRowView {
    /// 動作鍵，與 `MatrixCellView::chart_action` 同一組字串
    pub action: String,
    pub label: String,
    /// 這一列涵蓋的牌類代號。來源表寫「其餘手牌」的列已在載入時展開
    pub hands: Vec<String>,
    /// 這一列涵蓋的 combo 數（全部 1326 個之中）
    pub combos: u32,
    /// 佔 1326 個 combo 的比例，萬分比
    pub share_myriad: u32,
    /// 這一列若是加注，加注到多少（BB 的百分之一）
    pub raise_to_centi_bb: Option<u32>,
    /// 來源表 H 欄的原文
    pub note: String,
}

/// 一個節點的完整範圍矩陣。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct RangeMatrixView {
    /// 節點的唯一鍵（`9max/BTN/160-240/unopened`）
    pub node_key: String,
    pub seated: u8,
    pub hero: String,
    pub bucket: String,
    pub scenario: String,
    pub scenario_label: String,
    /// 169 格，依 `HandClass::index()` 遞增（列由 A 到 2）
    pub cells: Vec<MatrixCellView>,
    /// 範圍寬度：以 combo 加權的主動頻率（萬分比）
    pub width_myriad: u32,
    /// 混合格數（同一手牌有兩種以上行動）
    pub mixed_count: u32,
    /// 本節點被覆寫的格數
    pub override_count: u32,
    /// 本節點的主動行動描述（推入或加注到多少 BB）
    pub aggressive_action: String,
    /// 產生排序時假設的對手數（見 `baseline::expected_opponents`）
    pub expected_opponents: u8,
    /// 這個節點的內容來源：`chart`（顧問的預設組合表）或 `baseline`（參數產生器）
    pub source: String,
    /// 本節點用的是表上哪一檔深度。引擎有九檔 bucket、表只有四檔，
    /// 對應關係必須說出來，否則使用者不知道 40–70BB 看的是 35–50BB 那一欄
    pub chart_depth: Option<String>,
    /// 本節點用的是表上哪一欄情境
    pub chart_scenario: Option<String>,
    /// 表上這一格的五列原文。`source` 為 `baseline` 時是空的
    pub chart_rows: Vec<ChartRowView>,
}

/// 一格覆寫。節點鍵的欄位與引擎決策時查表用的完全相同。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CellOverrideView {
    pub seated: u8,
    pub hero: String,
    pub bucket: String,
    pub scenario: String,
    pub class: String,
    /// 主動頻率，萬分比
    pub aggressive: u32,
    /// 跟注頻率，萬分比。棄牌是餘數，因此不可能寫出合計不等於 100% 的覆寫
    pub call: u32,
}

/// 內容現況。
#[must_use]
pub fn meta() -> StrategyMetaView {
    let rules = BaselineRules::engineering_placeholder();
    let nodes = enumerate_nodes().len() as u64;
    // 排序的來源與等級隨 meta 一起送出。面板必須說得出自己畫的是什麼
    // 內容——低樣本替代品畫出來的矩陣與正式的長得一模一樣
    let status = crate::rankings::status();
    // 預設組合表的現況同樣要說出來。表載不進來時面板畫的是參數產生的
    // 佔位內容，那與顧問簽核過的東西不是同一份，使用者沒有辦法自己分辨
    let chart = DefaultChart::embedded();
    let (chart_cell_count, chart_coverage_myriad, chart_note) = match chart {
        Ok(chart) => {
            let covered = enumerate_nodes().iter().filter(|n| chart.covers(n)).count() as u64;
            let coverage = u32::try_from(covered * 10_000 / nodes.max(1)).unwrap_or(0);
            (
                chart.entries().len() as u64,
                coverage,
                "翻前一律照顧問的預設組合表。表沒有「面對跛入」那一欄，\
                 該情境仍走參數產生器；引擎的九檔籌碼分檔對應到表上的四檔深度。"
                    .to_owned(),
            )
        }
        Err(error) => (0, 0, format!("預設組合表載入失敗，已退回參數產生器：{error}")),
    };
    StrategyMetaView {
        chart_source: chart.map_or_else(|_| "（未載入）".to_owned(), |c| c.source.clone()),
        chart_version: CHART_VERSION.to_owned(),
        chart_loaded: chart.is_ok(),
        chart_cell_count,
        chart_coverage_myriad,
        chart_note,
        baseline_name: rules.name.clone(),
        baseline_version: rules.version.clone(),
        consultant_approved: rules.consultant_approved,
        push_fold_below: rules.push_fold_below.as_str().to_owned(),
        open_size_centi_bb: rules.open_size_centi_bb,
        three_bet_size_centi_bb: rules.three_bet_size_centi_bb,
        four_bet_size_centi_bb: rules.four_bet_size_centi_bb,
        postflop_fallback: poker_engine::bot::POSTFLOP_FALLBACK_VERSION.to_owned(),
        preflop_node_count: nodes,
        preflop_cell_count: nodes * 169,
        ranking_samples: status.samples,
        ranking_source: status.source,
        ranking_content_grade: status.content_grade,
        ranking_note: status.note,
    }
}

/// 某（桌型 × 位置）的導航選項。
///
/// 情境清單由引擎的 [`scenarios_for`] 產生，因此 UI 不會列出不可能到達
/// 的節點（UTG 不會「面對開牌」、BB 不會被 3-bet）。
#[must_use]
pub fn nodes(seated: u8, hero: &str) -> StrategyNodesView {
    let seated = seated.clamp(6, 9);
    let order = positions_for(seated);
    let hero_label = parse_position(hero)
        .filter(|p| order.contains(p))
        .unwrap_or_else(|| order[0]);
    let rules = BaselineRules::engineering_placeholder();

    StrategyNodesView {
        seated,
        positions: order.iter().map(|p| p.as_str().to_owned()).collect(),
        hero: hero_label.as_str().to_owned(),
        scenarios: scenarios_for(seated, hero_label)
            .into_iter()
            .map(|scenario| ScenarioOptionView {
                key: scenario.key(),
                label: scenario_label(scenario),
                group: scenario_group(scenario).to_owned(),
            })
            .collect(),
        buckets: all_buckets()
            .into_iter()
            .map(|bucket| BucketOptionView {
                key: bucket.as_str().to_owned(),
                label: bucket_label(bucket),
                push_fold: rules.is_push_fold(bucket),
                multiplier: rules.bucket_multiplier_of(bucket),
                chart_depth: ChartDepth::from_bucket(bucket).label().to_owned(),
            })
            .collect(),
    }
}

/// 產生一個節點的 13×13 矩陣。
///
/// `overrides` 是**整份**覆寫清單；不屬於本節點的項目不會命中，因此
/// 呼叫端不必先過濾。過濾放在呼叫端反而會多一處「節點鍵怎麼算」的
/// 實作，那正是要避免的漂移。
///
/// # Errors
/// 節點欄位無法解析，或該情境在此（桌型 × 位置）下不可能到達時回傳說明。
pub fn matrix(
    seated: u8,
    hero: &str,
    bucket: &str,
    scenario: &str,
    overrides: &[CellOverrideView],
) -> Result<RangeMatrixView, String> {
    let node = parse_node(seated, hero, bucket, scenario)?;
    let mut rules = BaselineRules::engineering_placeholder();
    rules.overrides = to_cell_overrides(overrides)?;
    build_matrix_view(node, &rules)
}

/// 某個 Bot 設定在指定節點會打出的範圍。
///
/// 面板 C 的即時預覽用。四支人格滑桿在走表的節點上是靠 `ChartShift`
/// 在內容層作用，光看參數數字看不出它們到底把哪幾手牌搬去哪裡。
///
/// 規則推導走引擎的 [`poker_engine::bot::rules_for_bot`]，與 `BotAgent`
/// 實際決策時同一支。這裡自己算一份的話，預覽畫的就不是 Bot 會打的東西。
///
/// # Errors
/// 節點欄位無法解析、情境在此桌型下不可能到達，或 Bot 參數越界時回傳說明。
pub fn bot_matrix(
    seated: u8,
    hero: &str,
    bucket: &str,
    scenario: &str,
    bot: &crate::bots::BotSeatConfig,
) -> Result<RangeMatrixView, String> {
    let node = parse_node(seated, hero, bucket, scenario)?;
    let config = bot.to_bot_config()?;
    let rules = poker_engine::bot::rules_for_bot(&BaselineRules::engineering_placeholder(), &config);
    build_matrix_view(node, &rules)
}

/// 節點 ＋ 生效規則 → 矩陣。`matrix` 與 `bot_matrix` 只差在規則怎麼來。
fn build_matrix_view(node: PreflopNode, rules: &BaselineRules) -> Result<RangeMatrixView, String> {
    let opponents = baseline::expected_opponents(&node);
    let ranking = crate::rankings::for_opponents(opponents)
        .map_err(|reason| format!("equity 排序內容不可用：{reason}"))?;
    let built = RangeMatrix::build(node, rules, ranking);

    // 這個節點的內容是表給的還是參數產生的。面板必須畫得出來：兩者
    // 一個是顧問寫的清單、一個是 equity 排序的啟發式，長相一樣但意義
    // 完全不同
    let entry = DefaultChart::embedded().ok().and_then(|chart| chart.lookup(&node));

    let cells: Vec<MatrixCellView> = HandClass::all()
        .into_iter()
        .map(|class| {
            let cell = built.cell(class);
            let (row, col) = class.grid();
            MatrixCellView {
                class: class.label(),
                row: u8::try_from(row).unwrap_or(0),
                col: u8::try_from(col).unwrap_or(0),
                combos: class.combos(),
                aggressive: cell.aggressive,
                call: cell.call,
                check: cell.check,
                fold: cell.fold,
                percentile: cell.percentile,
                overridden: rules.overrides.get(&node, class).is_some(),
                chart_action: entry
                    .and_then(|entry| entry.action_of(class))
                    .map(|action| action.as_str().to_owned()),
            }
        })
        .collect();

    Ok(RangeMatrixView {
        node_key: node.key(),
        seated: node.seated,
        hero: node.hero.as_str().to_owned(),
        bucket: node.bucket.as_str().to_owned(),
        scenario: node.scenario.key(),
        scenario_label: scenario_label(node.scenario),
        override_count: u32::try_from(cells.iter().filter(|c| c.overridden).count()).unwrap_or(0),
        mixed_count: u32::try_from(built.mixed_cells().len()).unwrap_or(0),
        width_myriad: built.width_myriad(),
        aggressive_action: aggressive_action(&node, rules),
        expected_opponents: u8::try_from(opponents).unwrap_or(1),
        source: if entry.is_some() { "chart" } else { "baseline" }.to_owned(),
        chart_depth: entry.map(|_| ChartDepth::from_bucket(node.bucket).label().to_owned()),
        chart_scenario: entry.map(|e| e.scenario.label().to_owned()),
        chart_rows: entry.map(chart_rows).unwrap_or_default(),
        cells,
    })
}

/// 把跨 IPC 的覆寫清單轉為引擎的覆寫表。
///
/// # Errors
/// 節點或牌類無法解析，或主動加跟注超過 100% 時回傳說明。**不靜默忽略**：
/// 使用者以為改了一格，引擎沒收到，跑出來的統計就對不上他看到的矩陣。
pub fn to_cell_overrides(views: &[CellOverrideView]) -> Result<CellOverrides, String> {
    let mut out = CellOverrides::new();
    for view in views {
        let node = parse_node(view.seated, &view.hero, &view.bucket, &view.scenario)?;
        let class = parse_class(&view.class)?;
        let cell = OverrideCell::new(view.aggressive, view.call).ok_or_else(|| {
            format!(
                "{} 的覆寫合計超過 100%（主動 {} ＋ 跟注 {} 萬分比）",
                view.class, view.aggressive, view.call
            )
        })?;
        out.set(node, class, cell);
    }
    Ok(out)
}

fn parse_node(
    seated: u8,
    hero: &str,
    bucket: &str,
    scenario: &str,
) -> Result<PreflopNode, String> {
    if !(6..=9).contains(&seated) {
        return Err(format!("座位數 {seated} 不在 6–9 之內"));
    }
    let order = positions_for(seated);
    let hero_label = parse_position(hero).ok_or_else(|| format!("未知的位置：{hero}"))?;
    if !order.contains(&hero_label) {
        return Err(format!("{seated} 人桌沒有 {hero} 這個位置"));
    }
    let bucket_value = parse_bucket(bucket).ok_or_else(|| format!("未知的籌碼分檔：{bucket}"))?;
    let scenario_value =
        parse_scenario(scenario).ok_or_else(|| format!("未知的情境：{scenario}"))?;
    if !scenarios_for(seated, hero_label).contains(&scenario_value) {
        return Err(format!(
            "{seated} 人桌的 {hero} 不可能遇到「{}」",
            scenario_label(scenario_value)
        ));
    }
    Ok(PreflopNode {
        seated,
        hero: hero_label,
        bucket: bucket_value,
        scenario: scenario_value,
    })
}

fn parse_position(text: &str) -> Option<PositionLabel> {
    use PositionLabel::{Bb, Btn, Co, Hj, Lj, Sb, Utg, Utg1, Utg2, Utg3, Utg4};
    [Utg, Utg1, Utg2, Utg3, Utg4, Lj, Hj, Co, Btn, Sb, Bb]
        .into_iter()
        .find(|p| p.as_str() == text)
}

fn parse_bucket(text: &str) -> Option<StackBucket> {
    all_buckets().into_iter().find(|b| b.as_str() == text)
}

/// 由 [`PreflopScenario::key`] 產生的字串反解回情境。
///
/// 兩邊必須對稱：鍵是內容表的欄位名，也是覆寫的節點鍵的一部分，
/// 解不回來就等於使用者的覆寫悄悄落在另一個節點上。
fn parse_scenario(text: &str) -> Option<PreflopScenario> {
    if text == "unopened" {
        return Some(PreflopScenario::Unopened);
    }
    if let Some(rest) = text.strip_prefix("vs-limp-") {
        return rest
            .parse()
            .ok()
            .map(|limpers| PreflopScenario::VsLimp { limpers });
    }
    // 這一段必須排在 `vs-open-` 之前：`vs-open-raise-UTG` 也以 `vs-open-`
    // 開頭，順序反過來會把它解析成位置「raise-UTG」而失敗，使用者的覆寫
    // 就悄悄落到別的節點上
    if let Some(rest) = text.strip_prefix("vs-open-raise-") {
        return parse_position(rest).map(|opener| PreflopScenario::VsOpenRaise { opener });
    }
    if let Some(rest) = text.strip_prefix("vs-open-") {
        return parse_position(rest).map(|opener| PreflopScenario::VsOpen { opener });
    }
    if let Some(rest) = text.strip_prefix("vs-3bet-") {
        return parse_position(rest).map(|by| PreflopScenario::VsThreeBet { by });
    }
    if let Some(rest) = text.strip_prefix("vs-4bet-") {
        return parse_position(rest).map(|by| PreflopScenario::VsFourBet { by });
    }
    if let Some(rest) = text.strip_prefix("vs-squeeze-") {
        return parse_position(rest).map(|by| PreflopScenario::VsSqueeze { by });
    }
    None
}

fn parse_class(text: &str) -> Result<HandClass, String> {
    HandClass::from_label(text).ok_or_else(|| format!("未知的牌類：{text}"))
}

fn scenario_label(scenario: PreflopScenario) -> String {
    match scenario {
        PreflopScenario::Unopened => "無人進池".to_owned(),
        PreflopScenario::VsLimp { limpers } => format!("面對 {limpers} 名跛入"),
        PreflopScenario::VsOpen { opener } => format!("面對 {} 開牌", opener.as_str()),
        PreflopScenario::VsOpenRaise { opener } => {
            format!("面對 {} 開牌後再被加注", opener.as_str())
        }
        PreflopScenario::VsThreeBet { by } => format!("被 {} 3-bet", by.as_str()),
        PreflopScenario::VsFourBet { by } => format!("被 {} 4-bet", by.as_str()),
        PreflopScenario::VsSqueeze { by } => format!("被 {} 擠壓", by.as_str()),
    }
}

const fn scenario_group(scenario: PreflopScenario) -> &'static str {
    match scenario {
        PreflopScenario::Unopened => "開牌",
        PreflopScenario::VsLimp { .. } => "面對跛入",
        PreflopScenario::VsOpen { .. } => "面對開牌",
        PreflopScenario::VsOpenRaise { .. } => "面對開牌＋再加注",
        PreflopScenario::VsThreeBet { .. } => "面對 3-bet",
        PreflopScenario::VsFourBet { .. } => "面對 4-bet",
        PreflopScenario::VsSqueeze { .. } => "面對擠壓",
    }
}

fn bucket_label(bucket: StackBucket) -> String {
    let key = bucket.as_str();
    match key.strip_suffix('+') {
        Some(head) => format!("{head}+ BB"),
        None => format!("{} BB", key.replace('-', "–")),
    }
}

/// 本節點的主動行動。
///
/// UI 不得自行推導加注金額（UI 規格 E.1），因此尺度由引擎給：短碼採
/// 推入，其餘走該情境的加注尺度。
fn aggressive_action(node: &PreflopNode, rules: &BaselineRules) -> String {
    // 走表的節點用表自己的尺度：表的倍數乘的是前方最大下注額，
    // 與參數產生器的 centi-BB 不是同一組數字
    if let Some(entry) = DefaultChart::embedded().ok().and_then(|c| c.lookup(node)) {
        return match entry.primary_aggressive() {
            Some(ChartAction::AllIn) => "全下".to_owned(),
            Some(action) => raise_size_centi_bb(entry.scenario, action).map_or_else(
                || "全下".to_owned(),
                |centi| format!("加注到 {:.2} BB", f64::from(centi) / 100.0),
            ),
            None => "此情境不主動下注".to_owned(),
        };
    }
    if rules.is_push_fold(node.bucket) {
        return "全下".to_owned();
    }
    let centi = baseline::raise_size(node.scenario, rules);
    format!("加注到 {:.2} BB", f64::from(centi) / 100.0)
}

/// 表上這一格的五列原文，供面板逐列顯示。
fn chart_rows(entry: &ChartEntry) -> Vec<ChartRowView> {
    entry
        .rows()
        .iter()
        .map(|row| ChartRowView {
            action: row.action.as_str().to_owned(),
            label: row.action.label().to_owned(),
            hands: row.classes().iter().map(|c| c.label()).collect(),
            combos: row.combos(),
            share_myriad: row.share_myriad(),
            raise_to_centi_bb: raise_size_centi_bb(entry.scenario, row.action),
            note: row.note.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::bots::BotSeatConfig;
    use std::collections::BTreeMap;

    fn 節點() -> (u8, &'static str, &'static str, &'static str) {
        (9, "BTN", "160-240", "unopened")
    }

    /// 參數全預設時，Bot 預覽必須與自身策略的矩陣逐格相同。
    ///
    /// 不同的話代表 `rules_for_bot` 在中性參數下不是空操作——那會讓
    /// 「沒調任何東西」的 Bot 打得跟表不一樣，而畫面上看不出來。
    #[test]
    fn 中性參數的_bot_預覽等於照表() {
        let (seated, hero, bucket, scenario) = 節點();
        let plain = matrix(seated, hero, bucket, scenario, &[]).expect("矩陣");
        let bot = bot_matrix(
            seated,
            hero,
            bucket,
            scenario,
            &BotSeatConfig {
                name: "標準".to_owned(),
                params: BTreeMap::new(),
            },
        )
        .expect("bot 矩陣");

        assert_eq!(plain.width_myriad, bot.width_myriad);
        for (a, b) in plain.cells.iter().zip(bot.cells.iter()) {
            assert_eq!(
                (a.class.as_str(), a.aggressive, a.call, a.check, a.fold),
                (b.class.as_str(), b.aggressive, b.call, b.check, b.fold),
                "{} 在中性參數下不該與照表不同",
                a.class
            );
        }
    }

    /// 收窄 rangeWidth 必須讓範圍變窄。滑桿拉了卻沒有變化，預覽就是裝飾品。
    #[test]
    fn 收窄範圍寬度會讓預覽變窄() {
        let (seated, hero, bucket, scenario) = 節點();
        let wide = bot_matrix(
            seated,
            hero,
            bucket,
            scenario,
            &BotSeatConfig {
                name: "寬".to_owned(),
                params: [("rangeWidth".to_owned(), 14_000)].into_iter().collect(),
            },
        )
        .expect("寬");
        let tight = bot_matrix(
            seated,
            hero,
            bucket,
            scenario,
            &BotSeatConfig {
                name: "緊".to_owned(),
                params: [("rangeWidth".to_owned(), 6_000)].into_iter().collect(),
            },
        )
        .expect("緊");

        assert!(
            tight.width_myriad < wide.width_myriad,
            "收窄後應更窄：緊 {} vs 寬 {}",
            tight.width_myriad,
            wide.width_myriad
        );
    }

    use super::*;
    use poker_engine::strategy::distribution::FULL;

    /// 面板 D 的第一次請求必須是即時的。
    ///
    /// 這條守的是實機回報的假死：`strategy_matrix` 是同步 Tauri command，
    /// 跑在**主執行緒**上；舊版的第一次呼叫要等內容級 equity 排序算完
    /// （debug 建置 80 秒），視窗因此整段沒有回應，Windows 判成 AppHangB1。
    ///
    /// 排序改由離線資產載入之後，這條路徑不得再有任何 Monte Carlo。
    #[test]
    fn 第一次取矩陣不得現算_equity_排序() {
        let start = std::time::Instant::now();
        let view = matrix(9, "BTN", "160-240", "unopened", &[]).expect("節點合法");
        let elapsed = start.elapsed();

        assert_eq!(view.cells.len(), 169);
        assert!(
            elapsed < std::time::Duration::from_secs(1),
            "第一次取矩陣花了 {elapsed:?}——這條路徑跑在主執行緒上，不得有現算"
        );
    }

    /// 面板必須說得出自己畫的是哪一份排序。
    #[test]
    fn 內容現況帶著排序來源與等級() {
        let meta = meta();
        assert_eq!(meta.ranking_source, "asset/v1");
        assert!(meta.ranking_content_grade);
        assert_eq!(meta.ranking_samples, 20_000);
        assert!(!meta.ranking_note.is_empty(), "說明不得是空字串");
    }

    #[test]
    fn 情境鍵可以來回轉換() {
        for seated in 6u8..=9 {
            for hero in positions_for(seated) {
                for scenario in scenarios_for(seated, hero) {
                    let key = scenario.key();
                    assert_eq!(
                        parse_scenario(&key),
                        Some(scenario),
                        "情境鍵 {key} 解不回原本的情境，覆寫會落到別的節點"
                    );
                }
            }
        }
    }

    #[test]
    fn 位置與籌碼分檔的鍵可以來回轉換() {
        for seated in 6u8..=9 {
            for hero in positions_for(seated) {
                assert_eq!(parse_position(hero.as_str()), Some(hero));
            }
        }
        for bucket in all_buckets() {
            assert_eq!(parse_bucket(bucket.as_str()), Some(bucket));
        }
    }

    #[test]
    fn 不可能到達的節點被拒絕() {
        // UTG 是最早行動者，不可能面對開牌
        let error = parse_node(9, "UTG", "160-240", "vs-open-LJ").unwrap_err();
        assert!(error.contains("不可能"), "錯誤訊息應說明原因：{error}");
        // 6 人桌沒有 UTG+1
        assert!(parse_node(6, "UTG+1", "160-240", "unopened").is_err());
    }

    #[test]
    fn 覆寫真的改變那一格而且只改那一格() {
        let base = matrix(9, "BTN", "160-240", "unopened", &[]).expect("節點合法");
        let overrides = vec![CellOverrideView {
            seated: 9,
            hero: "BTN".to_owned(),
            bucket: "160-240".to_owned(),
            scenario: "unopened".to_owned(),
            class: "72o".to_owned(),
            aggressive: FULL,
            call: 0,
        }];
        let edited = matrix(9, "BTN", "160-240", "unopened", &overrides).expect("節點合法");

        let find = |view: &RangeMatrixView, label: &str| {
            view.cells
                .iter()
                .find(|c| c.class == label)
                .expect("牌類存在")
                .clone()
        };

        let before = find(&base, "72o");
        let after = find(&edited, "72o");
        assert_eq!(before.aggressive, 0, "72o 在預設 BTN 開牌範圍外");
        assert_eq!(after.aggressive, FULL);
        assert!(after.overridden);
        assert_eq!(after.fold, 0);

        // 相鄰牌類不得被連帶改動
        assert_eq!(find(&base, "73o").aggressive, find(&edited, "73o").aggressive);
        assert!(!find(&edited, "73o").overridden);
        assert!(edited.width_myriad > base.width_myriad, "多開一格，寬度必須變大");
    }

    #[test]
    fn 覆寫不會滲進其他節點() {
        let overrides = vec![CellOverrideView {
            seated: 9,
            hero: "BTN".to_owned(),
            bucket: "160-240".to_owned(),
            scenario: "unopened".to_owned(),
            class: "72o".to_owned(),
            aggressive: FULL,
            call: 0,
        }];
        let other = matrix(9, "CO", "160-240", "unopened", &overrides).expect("節點合法");
        assert_eq!(
            other
                .cells
                .iter()
                .filter(|c| c.overridden)
                .count(),
            0,
            "BTN 的覆寫不得出現在 CO 的矩陣"
        );
    }

    #[test]
    fn 合計超過百分之百的覆寫被擋下() {
        let overrides = vec![CellOverrideView {
            seated: 9,
            hero: "BTN".to_owned(),
            bucket: "160-240".to_owned(),
            scenario: "unopened".to_owned(),
            class: "AA".to_owned(),
            aggressive: 7_000,
            call: 5_000,
        }];
        assert!(to_cell_overrides(&overrides).is_err());
    }

    #[test]
    fn 每格的三個頻率合計為百分之百() {
        let view = matrix(9, "HJ", "70-110", "unopened", &[]).expect("節點合法");
        assert_eq!(view.cells.len(), 169);
        for cell in &view.cells {
            assert_eq!(
                cell.aggressive + cell.call + cell.check + cell.fold,
                FULL,
                "{} 的頻率合計不是 100%",
                cell.class
            );
        }
    }

    #[test]
    fn 位置越晚開牌範圍越寬() {
        let width = |hero: &str| {
            matrix(9, hero, "160-240", "unopened", &[])
                .expect("節點合法")
                .width_myriad
        };
        assert!(
            width("UTG") < width("CO") && width("CO") < width("BTN"),
            "開牌寬度應隨位置變晚而增加"
        );
    }

    #[test]
    fn 短碼節點的主動行動是全下() {
        let short = matrix(9, "BTN", "0-15", "unopened", &[]).expect("節點合法");
        assert_eq!(short.aggressive_action, "全下");
        let deep = matrix(9, "BTN", "160-240", "unopened", &[]).expect("節點合法");
        assert!(deep.aggressive_action.starts_with("加注到"));
    }

    #[test]
    fn 導航只列出到得了的情境() {
        let utg = nodes(9, "UTG");
        assert!(utg.scenarios.iter().all(|s| !s.key.starts_with("vs-open-")));
        assert_eq!(utg.buckets.len(), 9);
        assert_eq!(utg.positions.len(), 9);

        let six = nodes(6, "UTG+1");
        assert_eq!(six.hero, "UTG", "6 人桌沒有 UTG+1，應落到第一個位置");
        assert_eq!(six.positions.len(), 6);
    }

    #[test]
    fn 內容現況如實揭露未簽核() {
        let view = meta();
        assert!(!view.consultant_approved, "工程佔位內容不得標成已簽核");
        assert_eq!(view.preflop_cell_count, view.preflop_node_count * 169);
        assert_eq!(view.postflop_fallback, "checkFold/v0");
    }

    // ── 預設組合表 ─────────────────────────────────────────────────────

    /// 面板必須說得出這一格是顧問寫的還是參數算的。
    ///
    /// 兩者畫出來長得一模一樣：一個是逐格的手牌清單、一個是 equity 排序
    /// 的啟發式。不標來源的話，使用者沒有任何辦法分辨自己在看什麼。
    #[test]
    fn 走預設組合表的節點標明來源並附上表的原文() {
        let view = matrix(9, "UTG", "0-15", "unopened", &[]).expect("節點合法");
        assert_eq!(view.source, "chart");
        assert_eq!(view.chart_depth.as_deref(), Some("0–15BB"));
        assert_eq!(view.chart_rows.len(), 5, "表上一格恆有五個動作");

        let shove = view
            .chart_rows
            .iter()
            .find(|r| r.action == "allin")
            .expect("有 ALL IN 那一列");
        assert_eq!(shove.combos, 108);
        assert!(shove.hands.contains(&"AA".to_owned()));
        assert!(!shove.note.is_empty(), "來源表 H 欄的原文不得在傳輸中丟掉");

        let aces = view.cells.iter().find(|c| c.class == "AA").expect("有 AA");
        assert_eq!(aces.chart_action.as_deref(), Some("allin"));
        let junk = view.cells.iter().find(|c| c.class == "72o").expect("有 72o");
        assert_eq!(junk.chart_action.as_deref(), Some("fold"));
    }

    /// 表沒有「面對跛入」那一欄，那些節點如實標成參數產生。
    #[test]
    fn 表未涵蓋的節點標為參數產生() {
        let view = matrix(9, "BTN", "70-110", "vs-limp-1", &[]).expect("節點合法");
        assert_eq!(view.source, "baseline");
        assert!(view.chart_rows.is_empty());
        assert!(view.chart_depth.is_none());
        assert!(view.cells.iter().all(|c| c.chart_action.is_none()));
    }

    /// 大盲在無人加注時是過牌，不是棄牌。
    ///
    /// 併進棄牌的話，面板會把「整張表都留下來看翻牌」畫成「什麼牌都丟」。
    #[test]
    fn 大盲無人加注畫成過牌而不是棄牌() {
        let view = matrix(9, "BB", "70-110", "unopened", &[]).expect("節點合法");
        assert_eq!(view.source, "chart");
        for cell in &view.cells {
            assert_eq!(cell.check, FULL, "{} 應為 100% 過牌", cell.class);
            assert_eq!(cell.fold, 0);
        }
    }

    /// 引擎九檔籌碼分檔各自對應到表上的哪一欄，導航要說得出來。
    #[test]
    fn 導航列出每一檔對應的表深度() {
        let view = nodes(9, "BTN");
        let depth_of = |key: &str| {
            view.buckets
                .iter()
                .find(|b| b.key == key)
                .map(|b| b.chart_depth.clone())
                .expect("該檔存在")
        };
        assert_eq!(depth_of("0-15"), "0–15BB");
        assert_eq!(depth_of("40-70"), "35–50BB");
        assert_eq!(depth_of("70-110"), "100BB");
        assert_eq!(depth_of("400+"), "200–250BB");
    }

    /// 面對「開牌＋再加注」是獨立的情境，導航要列得出來。
    #[test]
    fn 導航列出面對開牌加再加注的情境() {
        let view = nodes(9, "BTN");
        assert!(
            view.scenarios.iter().any(|s| s.key == "vs-open-raise-UTG"),
            "BTN 前方有多人，冷 4-bet 的情境到得了"
        );
        let early = nodes(9, "UTG+1");
        assert!(!early
            .scenarios
            .iter()
            .any(|s| s.key.starts_with("vs-open-raise-")));
        assert!(parse_node(9, "UTG+1", "70-110", "vs-open-raise-UTG").is_err());
    }

    /// 內容現況要帶著預設組合表的來源與涵蓋率。
    #[test]
    fn 內容現況帶著預設組合表的來源() {
        let view = meta();
        assert!(view.chart_loaded, "表必須載得進來");
        assert!(view.chart_source.contains("9MAX"));
        assert_eq!(view.chart_cell_count, 180);
        assert!(
            view.chart_coverage_myriad > 7_000,
            "表應涵蓋絕大多數翻前節點，實得 {}",
            view.chart_coverage_myriad
        );
        assert!(!view.chart_note.is_empty());
    }

    /// 走表的節點，主動行動的尺度要用表的倍數而不是參數的 centi-BB。
    #[test]
    fn 走表的節點用表自己的加注尺度() {
        let open = matrix(9, "BTN", "70-110", "unopened", &[]).expect("節點合法");
        assert_eq!(open.aggressive_action, "加注到 2.50 BB");
        let three_bet = matrix(9, "BTN", "70-110", "vs-open-CO", &[]).expect("節點合法");
        assert_eq!(three_bet.aggressive_action, "加注到 6.25 BB");
        let short = matrix(9, "BTN", "0-15", "unopened", &[]).expect("節點合法");
        assert_eq!(short.aggressive_action, "全下");
    }

    /// 覆寫仍然勝過表：使用者親手改的那一格最大。
    #[test]
    fn 覆寫勝過預設組合表() {
        let overrides = vec![CellOverrideView {
            seated: 9,
            hero: "UTG".to_owned(),
            bucket: "0-15".to_owned(),
            scenario: "unopened".to_owned(),
            class: "72o".to_owned(),
            aggressive: FULL,
            call: 0,
        }];
        let view = matrix(9, "UTG", "0-15", "unopened", &overrides).expect("節點合法");
        let cell = view.cells.iter().find(|c| c.class == "72o").expect("有 72o");
        assert_eq!(cell.aggressive, FULL);
        assert!(cell.overridden);
        // 表上仍然寫著蓋牌——面板要畫得出「你改掉了表的哪一格」
        assert_eq!(cell.chart_action.as_deref(), Some("fold"));
    }
}
