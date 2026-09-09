//! 翻後的未簽核工程通則（0907 計劃 Stage 2）。
//!
//! **這不是顧問內容，也不冒充 GTO。** 顧問的翻後規則還沒進來，但把整個
//! 翻後決策留給 equity heuristic 有一個結構性的問題：那條路徑吃的是
//! Monte Carlo 勝率，看不到牌力組、牌面雙軸與線路，因此使用者在策略頁
//! 上調的任何東西都不會影響它。
//!
//! 這一組通則是可編輯結構的第一份內容：依牌力組與下注狀態給出尺寸意圖
//! 頻率，讓規則引擎真的接進決策路徑。每一條都標記
//! `consultantApproved: false`，且排在使用者覆寫與官方內容之後。
//!
//! [`crate::bot::agent::POSTFLOP_BASELINE_VERSION`] 的 equity heuristic
//! 仍然保留：通則沒命中或合法動作被遮罩光時走它，它再不行才走
//! `checkFold/v0`。

use crate::strategy::distribution::Myriad;
use crate::strategy::postflop::{
    HandStrength, PostflopActionKind as Kind, PostflopCondition, PostflopIntentDistribution,
    PostflopRule, PostflopSituation, RuleSet, RuleSource,
};

/// 工程通則的內容版本。
pub const POSTFLOP_RULES_VERSION: &str = "handStrengthGeneric/v1-unapproved";

/// 建立未簽核的翻後通則規則集。
///
/// 每個牌力組在兩個下注狀態各一條，共 16 條。條件只指定牌力組與下注
/// 狀態——牌面、線路、位置、SPR 全部留空，因此它是名副其實的**通則**：
/// 任何更具體的規則（使用者覆寫、官方內容）都排在它前面。
#[must_use]
pub fn engineering_rules() -> RuleSet {
    let mut rules = Vec::with_capacity(16);
    for strength in HandStrength::ALL {
        rules.push(rule(strength, PostflopSituation::NoBet));
        rules.push(rule(strength, PostflopSituation::FacingBet));
    }
    RuleSet::new(rules, POSTFLOP_RULES_VERSION)
}

fn rule(strength: HandStrength, situation: PostflopSituation) -> PostflopRule {
    PostflopRule {
        id: format!("G-{}-{}", strength.key(), situation.key()),
        name: format!("{}／{}", strength.label(), situation.label()),
        condition: PostflopCondition {
            situation: Some(situation),
            hand_strength: Some(strength),
            ..PostflopCondition::default()
        },
        intent: intent(strength, situation),
        source: RuleSource::Generic,
        version: POSTFLOP_RULES_VERSION.to_owned(),
        consultant_approved: false,
    }
}

/// 各牌力組的尺寸意圖。
///
/// 三個共通取捨，寫在這裡免得看起來像隨手填的數字：
///
/// - **強牌不會 100% 下注。** 全部下注等於把過牌區間清空，對手看到過牌
///   就知道你沒牌；保留一成到兩成的過牌是為了讓過牌區間還有東西。
/// - **弱牌保留少量下注。** 完全不詐唬同樣是把資訊送出去，而且多人底池
///   的詐唬成功率低，因此比例壓得比單挑更低——但這一版的條件還沒有
///   人數維度，先統一用保守值。
/// - **面對下注的棄牌比例隨牌力單調上升。** 這是唯一在未校準前也該成立
///   的不變量，`未簽核通則的棄牌比例隨牌力單調` 測試釘住它。
fn intent(strength: HandStrength, situation: PostflopSituation) -> PostflopIntentDistribution {
    let weights: Vec<(Kind, Myriad)> = match (situation, strength) {
        // ── 無人下注 ────────────────────────────────────────────
        (PostflopSituation::NoBet, HandStrength::Nuts) => vec![
            (Kind::Check, 1_000),
            (Kind::ThirdPot, 1_500),
            (Kind::TwoThirdsPot, 4_500),
            (Kind::Pot, 3_000),
        ],
        (PostflopSituation::NoBet, HandStrength::MadePlusDraw) => vec![
            (Kind::Check, 1_500),
            (Kind::ThirdPot, 2_000),
            (Kind::TwoThirdsPot, 5_000),
            (Kind::Pot, 1_500),
        ],
        (PostflopSituation::NoBet, HandStrength::StrongMade) => vec![
            (Kind::Check, 2_000),
            (Kind::ThirdPot, 3_000),
            (Kind::TwoThirdsPot, 4_000),
            (Kind::Pot, 1_000),
        ],
        (PostflopSituation::NoBet, HandStrength::MediumMade) => vec![
            (Kind::Check, 5_500),
            (Kind::ThirdPot, 3_500),
            (Kind::TwoThirdsPot, 1_000),
        ],
        (PostflopSituation::NoBet, HandStrength::BluffCatcher) => {
            vec![(Kind::Check, 8_000), (Kind::ThirdPot, 2_000)]
        }
        (PostflopSituation::NoBet, HandStrength::StrongDraw) => vec![
            (Kind::Check, 4_000),
            (Kind::ThirdPot, 2_500),
            (Kind::TwoThirdsPot, 3_500),
        ],
        (PostflopSituation::NoBet, HandStrength::WeakDraw) => {
            vec![(Kind::Check, 7_500), (Kind::ThirdPot, 2_500)]
        }
        (PostflopSituation::NoBet, HandStrength::Air) => {
            vec![(Kind::Check, 8_500), (Kind::ThirdPot, 1_500)]
        }
        // ── 面對下注 ────────────────────────────────────────────
        (PostflopSituation::FacingBet, HandStrength::Nuts) => vec![
            (Kind::Call, 3_500),
            (Kind::TwoThirdsPot, 4_000),
            (Kind::Pot, 2_500),
        ],
        (PostflopSituation::FacingBet, HandStrength::MadePlusDraw) => vec![
            (Kind::Fold, 1_000),
            (Kind::Call, 6_000),
            (Kind::TwoThirdsPot, 3_000),
        ],
        (PostflopSituation::FacingBet, HandStrength::StrongMade) => vec![
            (Kind::Fold, 1_000),
            (Kind::Call, 6_500),
            (Kind::TwoThirdsPot, 2_500),
        ],
        (PostflopSituation::FacingBet, HandStrength::MediumMade) => {
            vec![(Kind::Fold, 3_500), (Kind::Call, 6_500)]
        }
        (PostflopSituation::FacingBet, HandStrength::BluffCatcher) => {
            vec![(Kind::Fold, 5_500), (Kind::Call, 4_500)]
        }
        (PostflopSituation::FacingBet, HandStrength::StrongDraw) => vec![
            (Kind::Fold, 2_500),
            (Kind::Call, 6_000),
            (Kind::TwoThirdsPot, 1_500),
        ],
        (PostflopSituation::FacingBet, HandStrength::WeakDraw) => {
            vec![(Kind::Fold, 7_000), (Kind::Call, 3_000)]
        }
        (PostflopSituation::FacingBet, HandStrength::Air) => {
            vec![(Kind::Fold, 8_500), (Kind::TwoThirdsPot, 1_500)]
        }
    };

    PostflopIntentDistribution::new(weights).expect("工程通則的權重合計必為 100%")
}
