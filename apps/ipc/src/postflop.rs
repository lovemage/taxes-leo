//! 面板 D 的翻後策略：可達節點、逐節點頻率與覆寫（0907 計劃 §6.1）。
//!
//! # 無狀態
//!
//! 後端不保存可變策略。UI 在本地維護兩份覆寫清單，查詢時整份送進來，
//! run 開始時以值凍結。沒有 `set_*`／`clear_*` command——那種介面會讓
//! 「畫面上看到的」與「run 實際用的」在兩個地方各自演化。
//!
//! # 為什麼節點清單不整份回傳
//!
//! 可達節點有 5900 個（見 `postflop_content_size` 精算）。整份序列化每次
//! 查詢都要來回近 1 MB，而 UI 的導覽是階層式的：街 → 下注狀態 → 線路 →
//! 牌面外觀 → 乾濕 → 牌力組 → 面對尺度。因此這裡回傳的是**各維度的選項**
//! 與節點總數，逐節點的頻率由 [`postflop_rule`] 單獨查。

use poker_engine::card::Card;
use poker_engine::chips::Chips;
use poker_engine::hand::Street;
use poker_engine::strategy::distribution::{Myriad, FULL};
use poker_engine::strategy::hand_strength::{classify, HandStrengthConfig};
use poker_engine::strategy::postflop::{
    classify_board, enumerate_postflop_nodes, BoardConnectivity, BoardSurface, FacingSize,
    HandStrength, Matched, PostflopActionKind, PostflopCondition, PostflopIntentDistribution,
    PostflopLineName, PostflopNode, PostflopRule, PostflopSituation, PostflopSizing, RuleIssue,
    RuleSet, RuleSource, NODE_SET_VERSION,
};
use poker_engine::strategy::postflop_baseline::engineering_rules;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 節點覆寫的規則 id 前綴。
///
/// 節點覆寫與規則覆寫同屬 `UserOverride` 層，但恢復語意完全不同
/// （計劃 §5.2），因此解析後必須分得出來是哪一種。
const NODE_OVERRIDE_PREFIX: &str = "node:";
/// 規則覆寫的 id 前綴。
const RULE_OVERRIDE_PREFIX: &str = "rule:";

// ── DTO ─────────────────────────────────────────────────────────────

/// 一個維度上的可選值。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PostflopOptionView {
    pub key: String,
    pub label: String,
    pub description: String,
}

/// 牌局線路選項。依下注狀態過濾，因此帶著自己屬於哪一種狀態。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PostflopLineOptionView {
    pub key: String,
    pub label: String,
    /// `no-bet` 或 `facing-bet`
    pub situation: String,
}

/// 一個動作欄位的頻率。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PostflopWeightView {
    pub kind: String,
    pub label: String,
    /// 萬分比
    pub myriad: u32,
    /// 這個動作在該下注狀態是否成立。不成立時固定為 0 且不可編輯
    pub available: bool,
    pub unavailable_reason: Option<String>,
}

/// 使用者輸入的一筆頻率。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PostflopWeightInput {
    /// `check`／`call`／`third-pot`／`two-thirds-pot`／`pot`／`fold`
    pub kind: String,
    pub myriad: u32,
}

/// 節點覆寫：鍵是完整的 canonical 節點鍵。
///
/// **UI 主編輯器只產生這一種。** 刪除它只影響那一個節點，
/// 驗收條件 10 因此成立。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PostflopNodeOverrideView {
    pub node_key: String,
    pub weights: Vec<PostflopWeightInput>,
}

/// 規則覆寫：鍵是條件集合，可以涵蓋一整片節點。
///
/// 由進階規則編輯器產生。刪除前必須顯示受影響的節點數——一條只指定
/// 牌力組的規則可能涵蓋數百個節點。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PostflopRuleOverrideView {
    pub id: String,
    /// 以下條件全部可省略；省略即萬用
    pub street: Option<String>,
    pub situation: Option<String>,
    pub line: Option<String>,
    pub surface: Option<String>,
    pub connectivity: Option<String>,
    pub hand_strength: Option<String>,
    pub facing_size: Option<String>,
    pub weights: Vec<PostflopWeightInput>,
}

/// 兩份覆寫清單。查詢與 run 都整份傳入。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PostflopOverridesView {
    #[serde(default)]
    pub nodes: Vec<PostflopNodeOverrideView>,
    #[serde(default)]
    pub rules: Vec<PostflopRuleOverrideView>,
}

/// 靜態節點覆蓋（策略頁用）。
///
/// 分母是可達節點數，與報表的執行期覆蓋是**兩個不同的指標**：策略頁在
/// 儲存前沒有 run，算不出「決策次數比例」。兩者都必須帶節點集合版本，
/// 否則跨版本比較沒有意義。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PostflopStaticCoverageView {
    pub node_set_version: String,
    #[ts(type = "number")]
    pub total_nodes: u64,
    /// 由使用者覆寫（節點或規則）決定的節點數
    #[ts(type = "number")]
    pub user: u64,
    #[ts(type = "number")]
    pub official: u64,
    #[ts(type = "number")]
    pub generic: u64,
    #[ts(type = "number")]
    pub fallback: u64,
    /// 玩家完整度（萬分比）＝ user ÷ total
    pub completeness_myriad: u32,
}

/// 面板 D 的翻後導覽選項與摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PostflopNodesView {
    pub node_set_version: String,
    pub streets: Vec<PostflopOptionView>,
    pub situations: Vec<PostflopOptionView>,
    pub lines: Vec<PostflopLineOptionView>,
    pub surfaces: Vec<PostflopOptionView>,
    pub connectivities: Vec<PostflopOptionView>,
    /// 依街別過濾：河牌只有五組
    pub hand_strengths: Vec<PostflopOptionView>,
    pub facing_sizes: Vec<PostflopOptionView>,
    pub coverage: PostflopStaticCoverageView,
    /// 未簽核提示
    pub consultant_approved: bool,
    pub note: String,
}

/// 查詢單一節點用的鍵。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PostflopRuleQuery {
    pub street: String,
    pub situation: String,
    pub line: String,
    pub surface: String,
    pub connectivity: String,
    pub hand_strength: String,
    pub facing_size: String,
}

/// 「恢復繼承值」按鈕的狀態（計劃 §5.2 方案 A）。
///
/// 只留一顆按鈕。四層鏈是 `使用者 → 官方 → 通則 → 工程 fallback`，
/// 刪掉使用者覆寫之後落到的就是官方值，因此「恢復官方預設」與「恢復
/// 繼承值」在現有模型下等價，留兩顆只會讓人以為它們不一樣。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PostflopRestoreView {
    /// 能不能就地清除
    pub enabled: bool,
    /// `node-override`／`rule-override`／`inherited`
    pub kind: String,
    /// 給使用者看的一句話
    pub reason: String,
    /// 規則覆寫時：那條規則的 id
    pub rule_id: Option<String>,
    /// 規則覆寫時：刪掉它會影響幾個節點
    #[ts(type = "number")]
    pub affected_nodes: u64,
}

/// 單一節點的解析結果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PostflopRuleView {
    pub node_key: String,
    pub weights: Vec<PostflopWeightView>,
    /// 合計，萬分比。必須是 10000 才能儲存
    pub total_myriad: u32,
    /// `user-node-override`／`user-rule-override`／`official`／`generic`／
    /// `engineering-fallback`
    pub source: String,
    pub source_label: String,
    pub rule_id: Option<String>,
    pub rule_name: String,
    pub consultant_approved: bool,
    pub restore: PostflopRestoreView,
}

/// 一條規則診斷。
///
/// UI 規格 D.8：只有 `error` 阻擋保存，`warning` 可保存但寫入驗證摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PostflopIssueView {
    /// `overlap`／`shadowed`／`unreachable`／`impossible`／
    /// `layer-order-violation`
    pub kind: String,
    /// `error` 或 `warning`
    pub severity: String,
    pub rule_id: String,
    pub rule_name: String,
    /// 規則的來源層，讓 UI 能把「你的規則」的問題排在前面
    pub source: String,
    pub message: String,
    /// 遮蔽與重疊會指向另一條規則
    pub related_rule_id: Option<String>,
}

/// 保存前的驗證摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PostflopDiagnosticsView {
    pub issues: Vec<PostflopIssueView>,
    #[ts(type = "number")]
    pub error_count: u64,
    #[ts(type = "number")]
    pub warning_count: u64,
    /// 有 error 就不得保存（UI 規格 D.8）
    pub can_save: bool,
}

/// 指定底牌的分類預覽。
///
/// 節點編輯器只顯示牌力組與條件；百分位、補牌數這些是**單手**的資料，
/// 放進節點欄位會讓人以為它們可以編輯。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PostflopHandPreviewView {
    pub hand_strength: String,
    pub hand_strength_label: String,
    pub surface: String,
    pub connectivity: String,
    pub made_category: String,
    pub improves_board: bool,
    pub board_partial_category: String,
    /// 對均勻隨機合法對手的百分位，萬分比
    pub percentile_myriad: u32,
    pub has_showdown_value: bool,
    pub raw_outs: u8,
    /// 折算後的有效補牌，百分之一張
    pub effective_outs_centi: u16,
    /// 其中能成同花或順子的部分
    pub draw_outs_centi: u16,
    pub nut_draw: bool,
    pub combo_draw: bool,
    pub backdoor_draw: bool,
    /// 對手模型。顧問校準門檻時必須看得到這一行
    pub opponent_model: String,
    pub consultant_approved: bool,
}

// ── 查詢 ─────────────────────────────────────────────────────────────

/// 導覽選項與靜態完整度。
///
/// `street` 決定牌力組清單：河牌沒有未來補牌，三個帶聽牌的組別不列出。
#[must_use]
pub fn postflop_nodes(street: &str, overrides: &PostflopOverridesView) -> PostflopNodesView {
    let street_value = parse_street(street).unwrap_or(Street::Flop);
    let rules = to_rule_set(overrides);

    PostflopNodesView {
        node_set_version: NODE_SET_VERSION.to_owned(),
        streets: [Street::Flop, Street::Turn, Street::River]
            .into_iter()
            .map(|street| PostflopOptionView {
                key: street_key(street).to_owned(),
                label: street_label(street).to_owned(),
                description: String::new(),
            })
            .collect(),
        situations: PostflopSituation::ALL
            .into_iter()
            .map(|situation| PostflopOptionView {
                key: situation.key().to_owned(),
                label: situation.label().to_owned(),
                description: String::new(),
            })
            .collect(),
        lines: PostflopLineName::NO_BET
            .into_iter()
            .chain(PostflopLineName::FACING_BET)
            .map(|line| PostflopLineOptionView {
                key: line.key().to_owned(),
                label: line.label().to_owned(),
                situation: line.situation().key().to_owned(),
            })
            .collect(),
        surfaces: BoardSurface::ALL
            .into_iter()
            .map(|surface| PostflopOptionView {
                key: surface.key().to_owned(),
                label: surface.label().to_owned(),
                description: surface.description().to_owned(),
            })
            .collect(),
        connectivities: BoardConnectivity::ALL
            .into_iter()
            .map(|connectivity| PostflopOptionView {
                key: connectivity.key().to_owned(),
                label: connectivity.label().to_owned(),
                description: connectivity.description().to_owned(),
            })
            .collect(),
        hand_strengths: HandStrength::for_street(street_value)
            .iter()
            .map(|strength| PostflopOptionView {
                key: strength.key().to_owned(),
                label: strength.label().to_owned(),
                description: strength.description().to_owned(),
            })
            .collect(),
        facing_sizes: facing_size_options(),
        coverage: static_coverage(&rules),
        consultant_approved: false,
        note: "翻後內容目前由未簽核的工程通則與 equity 基準產生，不是顧問簽核的策略。"
            .to_owned(),
    }
}

/// 單一節點的解析後頻率、來源與恢復語意。
///
/// # Errors
/// 節點鍵解析不出來，或指向不可達的節點時回傳說明。
pub fn postflop_rule(
    query: &PostflopRuleQuery,
    overrides: &PostflopOverridesView,
) -> Result<PostflopRuleView, String> {
    let node = parse_node(query)?;
    let rules = to_rule_set(overrides);
    let situation = node.situation;

    // 換算金額用一個固定的參考底池：這裡要的是**頻率**，不是實際籌碼。
    // 決策當下才用真實底池換算（見 `PostflopIntentDistribution::to_actions`）
    let sizing = PostflopSizing {
        pot: Chips::new(100),
        to_call: Chips::new(0),
    };
    let context = representative_context(&node);
    let (matched, _) = rules.resolve(&context, sizing, &|_| true);

    let (rule_index, source) = match matched {
        Matched::Rule { index, source } => (Some(index), source),
        Matched::Fallback(_) => (None, RuleSource::EngineeringFallback),
    };
    let rule = rule_index.and_then(|index| rules.rules().get(index));

    let weights = rule.map_or_else(
        || fallback_weights(situation),
        |rule| intent_weights(&rule.intent, situation),
    );
    let total_myriad = weights.iter().map(|weight| weight.myriad).sum();

    let is_node_override = rule
        .map(|rule| rule.id.starts_with(NODE_OVERRIDE_PREFIX))
        .unwrap_or(false);
    let is_rule_override = rule
        .map(|rule| rule.id.starts_with(RULE_OVERRIDE_PREFIX))
        .unwrap_or(false);

    let source_key = if is_node_override {
        "user-node-override"
    } else if is_rule_override {
        "user-rule-override"
    } else {
        source.key()
    };

    let restore = if is_node_override {
        PostflopRestoreView {
            enabled: true,
            kind: "node-override".to_owned(),
            reason: "刪除這一筆節點覆寫，值落回下一層。只影響這一個節點。".to_owned(),
            rule_id: rule.map(|rule| strip_prefix(&rule.id)),
            affected_nodes: 1,
        }
    } else if is_rule_override {
        let rule_id = rule.map(|rule| strip_prefix(&rule.id)).unwrap_or_default();
        let affected = affected_node_count(&rules, rule_index);
        PostflopRestoreView {
            enabled: false,
            kind: "rule-override".to_owned(),
            reason: format!(
                "這個值來自你的規則 {rule_id}，它涵蓋 {affected} 個節點。\
                 就地清除會連帶影響其他節點，因此請前往規則編輯，或以節點覆寫蓋過。"
            ),
            rule_id: Some(rule_id),
            affected_nodes: affected,
        }
    } else {
        PostflopRestoreView {
            enabled: false,
            kind: "inherited".to_owned(),
            reason: "此節點沒有你的覆寫。".to_owned(),
            rule_id: rule.map(|rule| rule.id.clone()),
            affected_nodes: 0,
        }
    };

    Ok(PostflopRuleView {
        node_key: node.key(),
        weights,
        total_myriad,
        source: source_key.to_owned(),
        source_label: source_label(source_key).to_owned(),
        rule_id: rule.map(|rule| strip_prefix(&rule.id)),
        rule_name: rule.map_or_else(
            || "工程 fallback（equity 基準）".to_owned(),
            |rule| rule.name.clone(),
        ),
        consultant_approved: rule.is_some_and(|rule| rule.consultant_approved),
        restore,
    })
}

/// 指定底牌與公共牌的分類預覽。
///
/// # Errors
/// 牌張解析失敗、張數不合法或牌張重複時回傳說明。
pub fn classify_postflop_hand(hole: &str, board: &str) -> Result<PostflopHandPreviewView, String> {
    let hole_cards = parse_cards(hole)?;
    let board_cards = parse_cards(board)?;

    if hole_cards.len() != 2 {
        return Err(format!("底牌必須是 2 張，收到 {}", hole_cards.len()));
    }
    if !(3..=5).contains(&board_cards.len()) {
        return Err(format!(
            "公共牌必須是 3～5 張，收到 {}",
            board_cards.len()
        ));
    }
    let mut all = board_cards.clone();
    all.extend_from_slice(&hole_cards);
    for (index, card) in all.iter().enumerate() {
        if all[..index].contains(card) {
            return Err("底牌與公共牌不得重複".to_owned());
        }
    }

    let textures = classify_board(&board_cards).ok_or("公共牌張數不合法")?;
    let config = HandStrengthConfig::ENGINEERING;
    let snapshot = classify([hole_cards[0], hole_cards[1]], &board_cards, &config);

    Ok(PostflopHandPreviewView {
        hand_strength: snapshot.group.key().to_owned(),
        hand_strength_label: snapshot.group.label().to_owned(),
        surface: textures.surface().key().to_owned(),
        connectivity: textures.connectivity().key().to_owned(),
        made_category: format!("{:?}", snapshot.made_category),
        improves_board: snapshot.improves_board,
        board_partial_category: format!("{:?}", snapshot.board_partial_category),
        percentile_myriad: snapshot.random_opponent_percentile_myriad,
        has_showdown_value: snapshot.has_showdown_value,
        raw_outs: snapshot.raw_outs,
        effective_outs_centi: snapshot.effective_outs_centi,
        draw_outs_centi: snapshot.draw_outs_centi,
        nut_draw: snapshot.nut_draw,
        combo_draw: snapshot.combo_draw,
        backdoor_draw: snapshot.backdoor_draw,
        opponent_model: HandStrengthConfig::OPPONENT_MODEL.to_owned(),
        consultant_approved: config.consultant_approved,
    })
}

/// 保存前的規則診斷。
///
/// 靜態檢查（矛盾條件、同層遮蔽與重疊、來源排序）與可達性檢查
/// （命不中任何可達節點）一起跑。可達性是 O(規則數 × 節點數)，因此
/// 只在這一支跑，不在每次編輯時跑。
#[must_use]
pub fn postflop_diagnostics(overrides: &PostflopOverridesView) -> PostflopDiagnosticsView {
    let rules = to_rule_set(overrides);
    let nodes = enumerate_postflop_nodes();

    let mut issues: Vec<PostflopIssueView> = rules
        .analyse()
        .into_iter()
        .chain(rules.analyse_reachability(&nodes))
        .filter_map(|issue| to_issue_view(&rules, issue))
        .collect();

    // 使用者自己的問題排前面：工程通則的警告對他來說是雜訊
    issues.sort_by_key(|issue| match issue.source.as_str() {
        "user-override" => 0,
        "official" => 1,
        "generic" => 2,
        _ => 3,
    });

    let error_count = issues
        .iter()
        .filter(|issue| issue.severity == "error")
        .count() as u64;
    let warning_count = u64::try_from(issues.len()).unwrap_or(u64::MAX) - error_count;

    PostflopDiagnosticsView {
        issues,
        error_count,
        warning_count,
        can_save: error_count == 0,
    }
}

fn to_issue_view(rules: &RuleSet, issue: RuleIssue) -> Option<PostflopIssueView> {
    let rule = rules.rules().get(issue.rule())?;
    let related = |index: usize| rules.rules().get(index).map(|other| strip_prefix(&other.id));

    let (kind, message, related_rule_id) = match issue {
        RuleIssue::Overlap { with, .. } => (
            "overlap",
            format!(
                "與同一層的規則 {} 部分重疊。先特例後通則時這是正常的。",
                related(with).unwrap_or_default()
            ),
            related(with),
        ),
        RuleIssue::Shadowed { by, .. } => (
            "shadowed",
            format!(
                "被同一層更早的規則 {} 完全涵蓋，永遠不會命中。",
                related(by).unwrap_or_default()
            ),
            related(by),
        ),
        RuleIssue::Unreachable { .. } => (
            "unreachable",
            "條件命不中任何可達節點。可能是條件寫錯，也可能是節點集合改版。".to_owned(),
            None,
        ),
        RuleIssue::Impossible { .. } => (
            "impossible",
            "條件本身不可能成立（範圍為空，或下注狀態與面對尺度矛盾）。".to_owned(),
            None,
        ),
        RuleIssue::LayerOrderViolation { after, .. } => (
            "layer-order-violation",
            format!(
                "排在來源層級較低的規則 {} 之後，你的覆寫會被它蓋掉。",
                related(after).unwrap_or_default()
            ),
            related(after),
        ),
    };

    Some(PostflopIssueView {
        kind: kind.to_owned(),
        severity: if issue.is_error() { "error" } else { "warning" }.to_owned(),
        rule_id: strip_prefix(&rule.id),
        rule_name: rule.name.clone(),
        source: match rule.source {
            RuleSource::UserOverride => "user-override",
            RuleSource::Official => "official",
            RuleSource::Generic => "generic",
            RuleSource::EngineeringFallback => "engineering-fallback",
        }
        .to_owned(),
        message,
        related_rule_id,
    })
}

/// 把兩份覆寫清單接到工程通則之前，組成完整的四層規則集。
///
/// 順序固定：**節點覆寫 → 規則覆寫 → 官方 → 通則 → 工程 fallback**。
/// 節點覆寫排在規則覆寫之前，因為它更具體；兩者同屬 `UserOverride` 層。
/// 官方層目前沒有內容（顧問尚未交付），因此鏈上只有三段有東西。
#[must_use]
pub fn to_rule_set(overrides: &PostflopOverridesView) -> RuleSet {
    let mut rules: Vec<PostflopRule> = Vec::new();

    for node_override in &overrides.nodes {
        let Some(node) = parse_node_key(&node_override.node_key) else {
            continue;
        };
        let Some(intent) = to_intent(&node_override.weights) else {
            continue;
        };
        rules.push(PostflopRule {
            id: format!("{NODE_OVERRIDE_PREFIX}{}", node_override.node_key),
            name: format!("你的節點覆寫（{}）", node.hand_strength.label()),
            condition: node.condition(),
            intent,
            source: RuleSource::UserOverride,
            version: "user/node".to_owned(),
            consultant_approved: false,
        });
    }

    for rule_override in &overrides.rules {
        let Some(intent) = to_intent(&rule_override.weights) else {
            continue;
        };
        rules.push(PostflopRule {
            id: format!("{RULE_OVERRIDE_PREFIX}{}", rule_override.id),
            name: format!("你的規則 {}", rule_override.id),
            condition: to_condition(rule_override),
            intent,
            source: RuleSource::UserOverride,
            version: "user/rule".to_owned(),
            consultant_approved: false,
        });
    }

    let baseline = engineering_rules();
    rules.extend(baseline.rules().iter().cloned());
    RuleSet::new(rules, baseline.fallback_version)
}

// ── 內部 ─────────────────────────────────────────────────────────────

/// 逐節點掃過整個節點集合，統計每個節點會落到哪一層。
fn static_coverage(rules: &RuleSet) -> PostflopStaticCoverageView {
    let nodes = enumerate_postflop_nodes();
    let sizing = PostflopSizing {
        pot: Chips::new(100),
        to_call: Chips::new(0),
    };

    let mut user = 0u64;
    let mut official = 0u64;
    let mut generic = 0u64;
    let mut fallback = 0u64;

    for node in &nodes {
        let context = representative_context(node);
        match rules.resolve(&context, sizing, &|_| true).0 {
            Matched::Rule { source, .. } => match source {
                RuleSource::UserOverride => user += 1,
                RuleSource::Official => official += 1,
                RuleSource::Generic => generic += 1,
                RuleSource::EngineeringFallback => fallback += 1,
            },
            Matched::Fallback(_) => fallback += 1,
        }
    }

    let total = u64::try_from(nodes.len()).unwrap_or(u64::MAX);
    PostflopStaticCoverageView {
        node_set_version: NODE_SET_VERSION.to_owned(),
        total_nodes: total,
        user,
        official,
        generic,
        fallback,
        completeness_myriad: (user * u64::from(FULL))
            .checked_div(total)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0),
    }
}

/// 一條規則涵蓋幾個可達節點。
fn affected_node_count(rules: &RuleSet, index: Option<usize>) -> u64 {
    let Some(rule) = index.and_then(|index| rules.rules().get(index)) else {
        return 0;
    };
    enumerate_postflop_nodes()
        .iter()
        .filter(|node| rule.condition.intersects(&node.condition()))
        .count()
        .try_into()
        .unwrap_or(u64::MAX)
}

/// 節點的代表性 context。
///
/// 節點沒有指定位置、人數、SPR 這些「更多條件」維度，因此填入一組不會
/// 被通則排除的中性值。規則若在那些維度上加了條件，就不會命中這個
/// 代表值——那正是「更多條件」的意思。
fn representative_context(node: &PostflopNode) -> poker_engine::strategy::postflop::PostflopContext {
    use poker_engine::position::PositionLabel;
    use poker_engine::strategy::decision::StackBucket;
    use poker_engine::strategy::postflop::{BoardTextures, PostflopContext, PotType};

    PostflopContext {
        street: node.street,
        board_textures: BoardTextures::new(node.surface, node.connectivity),
        hand_strength: node.hand_strength,
        active_players: 2,
        hero_position: PositionLabel::Btn,
        opponents_behind: 0,
        pot_type: PotType::SingleRaised,
        facing_size: node.facing_size,
        spr_centi: 400,
        effective_stack_bucket: StackBucket::Deeper,
        line: node.line.representative_line(),
    }
}

fn intent_weights(
    intent: &PostflopIntentDistribution,
    situation: PostflopSituation,
) -> Vec<PostflopWeightView> {
    PostflopActionKind::ALL
        .into_iter()
        .map(|kind| {
            let available = kind.is_available(situation);
            PostflopWeightView {
                kind: kind.key().to_owned(),
                label: kind.label(situation).to_owned(),
                myriad: if available { intent.weight_of(kind) } else { 0 },
                available,
                unavailable_reason: (!available).then(|| unavailable_reason(kind, situation)),
            }
        })
        .collect()
}

/// 沒有任何規則命中時顯示的欄位：全部 0，讓 UI 看得出這是缺口。
fn fallback_weights(situation: PostflopSituation) -> Vec<PostflopWeightView> {
    PostflopActionKind::ALL
        .into_iter()
        .map(|kind| {
            let available = kind.is_available(situation);
            PostflopWeightView {
                kind: kind.key().to_owned(),
                label: kind.label(situation).to_owned(),
                myriad: 0,
                available,
                unavailable_reason: (!available).then(|| unavailable_reason(kind, situation)),
            }
        })
        .collect()
}

fn unavailable_reason(kind: PostflopActionKind, situation: PostflopSituation) -> String {
    match (situation, kind) {
        (PostflopSituation::NoBet, PostflopActionKind::Call) => "無人下注時沒有可跟的注".to_owned(),
        (PostflopSituation::NoBet, PostflopActionKind::Fold) => {
            "無人下注時蓋牌沒有意義，過牌不用花錢".to_owned()
        }
        (PostflopSituation::FacingBet, PostflopActionKind::Check) => {
            "面對下注時不能過牌".to_owned()
        }
        _ => "此下注狀態不成立".to_owned(),
    }
}

fn to_intent(weights: &[PostflopWeightInput]) -> Option<PostflopIntentDistribution> {
    let parsed: Vec<(PostflopActionKind, Myriad)> = weights
        .iter()
        .filter_map(|weight| Some((parse_action_kind(&weight.kind)?, weight.myriad)))
        .collect();
    PostflopIntentDistribution::new(parsed).ok()
}

fn to_condition(rule: &PostflopRuleOverrideView) -> PostflopCondition {
    PostflopCondition {
        street: rule.street.as_deref().and_then(parse_street),
        situation: rule.situation.as_deref().and_then(parse_situation),
        board_surface: rule.surface.as_deref().and_then(parse_surface),
        board_connectivity: rule.connectivity.as_deref().and_then(parse_connectivity),
        hand_strength: rule.hand_strength.as_deref().and_then(parse_hand_strength),
        facing_size: rule.facing_size.as_deref().and_then(parse_facing_size),
        line: rule
            .line
            .as_deref()
            .and_then(parse_line)
            .map(PostflopLineName::minimal_condition)
            .unwrap_or_default(),
        ..PostflopCondition::default()
    }
}

fn parse_node(query: &PostflopRuleQuery) -> Result<PostflopNode, String> {
    let node = PostflopNode {
        street: parse_street(&query.street).ok_or_else(|| format!("未知的街別 {}", query.street))?,
        situation: parse_situation(&query.situation)
            .ok_or_else(|| format!("未知的下注狀態 {}", query.situation))?,
        line: parse_line(&query.line).ok_or_else(|| format!("未知的線路 {}", query.line))?,
        surface: parse_surface(&query.surface)
            .ok_or_else(|| format!("未知的牌面外觀 {}", query.surface))?,
        connectivity: parse_connectivity(&query.connectivity)
            .ok_or_else(|| format!("未知的順子結構 {}", query.connectivity))?,
        hand_strength: parse_hand_strength(&query.hand_strength)
            .ok_or_else(|| format!("未知的牌力組 {}", query.hand_strength))?,
        facing_size: parse_facing_size(&query.facing_size)
            .ok_or_else(|| format!("未知的面對尺度 {}", query.facing_size))?,
    };

    // 不可達的節點不給編輯：河牌的聽牌組別、翻牌的三條濕潤面都不存在
    if !enumerate_postflop_nodes().contains(&node) {
        return Err(format!("節點不可達：{}", node.key()));
    }
    Ok(node)
}

fn parse_node_key(key: &str) -> Option<PostflopNode> {
    let parts: Vec<&str> = key.split('|').collect();
    if parts.len() != 7 {
        return None;
    }
    let node = PostflopNode {
        street: parse_street(parts[0])?,
        situation: parse_situation(parts[1])?,
        line: parse_line(parts[2])?,
        surface: parse_surface(parts[3])?,
        connectivity: parse_connectivity(parts[4])?,
        hand_strength: parse_hand_strength(parts[5])?,
        facing_size: parse_facing_size(parts[6])?,
    };
    Some(node)
}

fn strip_prefix(id: &str) -> String {
    id.strip_prefix(NODE_OVERRIDE_PREFIX)
        .or_else(|| id.strip_prefix(RULE_OVERRIDE_PREFIX))
        .unwrap_or(id)
        .to_owned()
}

fn facing_size_options() -> Vec<PostflopOptionView> {
    [
        (FacingSize::None, "無人下注"),
        (FacingSize::Third, "1/3 底池"),
        (FacingSize::TwoThirds, "2/3 底池"),
        (FacingSize::Pot, "1 個底池"),
    ]
    .into_iter()
    .map(|(size, label)| PostflopOptionView {
        key: facing_size_key(size).to_owned(),
        label: label.to_owned(),
        description: String::new(),
    })
    .collect()
}

fn parse_cards(text: &str) -> Result<Vec<Card>, String> {
    text.split_whitespace()
        .map(|token| Card::parse(token).ok_or_else(|| format!("無法解析牌張 {token}")))
        .collect()
}

const fn street_key(street: Street) -> &'static str {
    match street {
        Street::Preflop => "preflop",
        Street::Flop => "flop",
        Street::Turn => "turn",
        Street::River => "river",
    }
}

const fn street_label(street: Street) -> &'static str {
    match street {
        Street::Preflop => "翻前",
        Street::Flop => "翻牌",
        Street::Turn => "轉牌",
        Street::River => "河牌",
    }
}

fn parse_street(key: &str) -> Option<Street> {
    match key {
        "flop" => Some(Street::Flop),
        "turn" => Some(Street::Turn),
        "river" => Some(Street::River),
        _ => None,
    }
}

fn parse_situation(key: &str) -> Option<PostflopSituation> {
    PostflopSituation::ALL
        .into_iter()
        .find(|situation| situation.key() == key)
}

fn parse_line(key: &str) -> Option<PostflopLineName> {
    PostflopLineName::NO_BET
        .into_iter()
        .chain(PostflopLineName::FACING_BET)
        .find(|line| line.key() == key)
}

fn parse_surface(key: &str) -> Option<BoardSurface> {
    BoardSurface::ALL
        .into_iter()
        .find(|surface| surface.key() == key)
}

fn parse_connectivity(key: &str) -> Option<BoardConnectivity> {
    BoardConnectivity::ALL
        .into_iter()
        .find(|connectivity| connectivity.key() == key)
}

fn parse_hand_strength(key: &str) -> Option<HandStrength> {
    HandStrength::ALL
        .into_iter()
        .find(|strength| strength.key() == key)
}

fn parse_action_kind(key: &str) -> Option<PostflopActionKind> {
    PostflopActionKind::ALL
        .into_iter()
        .find(|kind| kind.key() == key)
}

const fn facing_size_key(size: FacingSize) -> &'static str {
    match size {
        FacingSize::None => "none",
        FacingSize::Quarter => "quarter",
        FacingSize::Third => "third",
        FacingSize::Half => "half",
        FacingSize::TwoThirds => "two-thirds",
        FacingSize::ThreeQuarters => "three-quarters",
        FacingSize::Pot => "pot",
        FacingSize::Overbet => "overbet",
        FacingSize::AllIn => "all-in",
    }
}

fn parse_facing_size(key: &str) -> Option<FacingSize> {
    [
        FacingSize::None,
        FacingSize::Quarter,
        FacingSize::Third,
        FacingSize::Half,
        FacingSize::TwoThirds,
        FacingSize::ThreeQuarters,
        FacingSize::Pot,
        FacingSize::Overbet,
        FacingSize::AllIn,
    ]
    .into_iter()
    .find(|size| facing_size_key(*size) == key)
}

fn source_label(source: &str) -> &'static str {
    match source {
        "user-node-override" => "你的節點覆寫",
        "user-rule-override" => "你的規則",
        "official" => "官方內容",
        "generic" => "同組通則",
        _ => "工程 fallback",
    }
}
