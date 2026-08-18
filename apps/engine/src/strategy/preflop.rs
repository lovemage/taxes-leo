//! Preflop 策略節點與策略表。
//!
//! 核心規格 4.1：資料模型為 `node × handClass × action × size → frequency`，
//! 且 `node` 至少包含桌型人數、使用者位置、有效籌碼 bucket、ante、rake、
//! straddle 與**完整公開翻前 action history**。
//!
//! 「完整 action history」是內容體積爆炸的來源：`vs Open` 不是一個節點，
//! 而是「被 UTG 開牌」「被 CO 開牌」等多個不同節點。實做計劃 M0 閘門 2
//! 要求精算格數，[`enumerate_nodes`] 即為該精算的依據。

use crate::position::PositionLabel;
use crate::strategy::decision::StackBucket;

/// 翻前情境。對應 UI 規格 D.4 的節點情境清單。
///
/// 帶位置參數的變體是刻意的：核心規格 4.1 要求節點涵蓋完整公開行動史，
/// 因此「面對誰的開牌」屬於節點識別的一部分，不得合併。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PreflopScenario {
    /// 前面全部棄牌
    Unopened,
    /// 面對 n 名跛入者
    VsLimp { limpers: u8 },
    /// 面對某位置的開牌加注
    VsOpen { opener: PositionLabel },
    /// 英雄開牌後被某位置 3-bet
    VsThreeBet { by: PositionLabel },
    /// 英雄 3-bet 後被 4-bet
    VsFourBet { by: PositionLabel },
    /// 英雄開牌、有人跟注後被擠壓
    VsSqueeze { by: PositionLabel },
}

impl PreflopScenario {
    /// 內容表的欄位鍵。
    #[must_use]
    pub fn key(self) -> String {
        match self {
            Self::Unopened => "unopened".to_owned(),
            Self::VsLimp { limpers } => format!("vs-limp-{limpers}"),
            Self::VsOpen { opener } => format!("vs-open-{}", opener.as_str()),
            Self::VsThreeBet { by } => format!("vs-3bet-{}", by.as_str()),
            Self::VsFourBet { by } => format!("vs-4bet-{}", by.as_str()),
            Self::VsSqueeze { by } => format!("vs-squeeze-{}", by.as_str()),
        }
    }
}

/// 一個翻前決策節點。
///
/// ante／rake／straddle 也是核心規格 4.1 的節點要素，但它們是**桌型層級**
/// 的設定而非逐節點變化，因此保存在策略表的 meta 而不重複進每個節點鍵，
/// 避免內容量再乘上一個維度。策略表綁定桌型（UI 規格 D.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PreflopNode {
    pub seated: u8,
    pub hero: PositionLabel,
    pub bucket: StackBucket,
    pub scenario: PreflopScenario,
}

impl PreflopNode {
    /// 內容表的唯一鍵。
    #[must_use]
    pub fn key(&self) -> String {
        format!(
            "{}max/{}/{}/{}",
            self.seated,
            self.hero.as_str(),
            self.bucket.as_str(),
            self.scenario.key()
        )
    }
}

/// 各桌型由早到晚的位置序列（規則細則 8.4.1，無 dead 位的情形）。
#[must_use]
pub fn positions_for(seated: u8) -> Vec<PositionLabel> {
    use PositionLabel::{Bb, Btn, Co, Hj, Lj, Sb, Utg, Utg1, Utg2};
    match seated {
        6 => vec![Utg, Hj, Co, Btn, Sb, Bb],
        7 => vec![Utg, Lj, Hj, Co, Btn, Sb, Bb],
        8 => vec![Utg, Utg1, Lj, Hj, Co, Btn, Sb, Bb],
        9 => vec![Utg, Utg1, Utg2, Lj, Hj, Co, Btn, Sb, Bb],
        _ => Vec::new(),
    }
}

/// 全部 9 個有效籌碼分檔（規則細則 8.5）。
#[must_use]
pub fn all_buckets() -> Vec<StackBucket> {
    vec![
        StackBucket::VeryShort,
        StackBucket::Short,
        StackBucket::Medium,
        StackBucket::Deep,
        StackBucket::Deeper,
        StackBucket::Deepest,
        StackBucket::VeryDeep,
        StackBucket::UltraDeep,
        StackBucket::Unbounded,
    ]
}

/// 列舉某桌型下英雄在某位置的所有合法翻前情境。
///
/// 合法性依行動順序判定，例如 UTG 不可能「面對 UTG 開牌」，
/// BB 不可能被更晚的位置開牌。列舉不合法節點會虛增內容量。
#[must_use]
pub fn scenarios_for(seated: u8, hero: PositionLabel) -> Vec<PreflopScenario> {
    let order = positions_for(seated);
    let Some(hero_index) = order.iter().position(|&p| p == hero) else {
        return Vec::new();
    };
    let earlier = &order[..hero_index];
    let later = &order[hero_index + 1..];

    let mut out = vec![PreflopScenario::Unopened];

    // 面對跛入：需有更早的位置
    if !earlier.is_empty() {
        out.push(PreflopScenario::VsLimp { limpers: 1 });
        if earlier.len() >= 2 {
            out.push(PreflopScenario::VsLimp { limpers: 2 });
        }
    }

    // 面對開牌：開牌者必須在英雄之前
    for &opener in earlier {
        out.push(PreflopScenario::VsOpen { opener });
    }

    // 英雄開牌後被 3-bet：3-bet 者必須在英雄之後
    for &by in later {
        out.push(PreflopScenario::VsThreeBet { by });
        out.push(PreflopScenario::VsSqueeze { by });
    }

    // 英雄 3-bet 後被 4-bet：原開牌者在英雄之前
    for &by in earlier {
        out.push(PreflopScenario::VsFourBet { by });
    }

    out
}

/// 列舉全部翻前節點（6～9 人 × 位置 × bucket × 情境）。
///
/// 這是實做計劃 M0 閘門 2 所需的「格數清單」依據：
/// 節點數 × 169 即為 baseline 的總格數。
#[must_use]
pub fn enumerate_nodes() -> Vec<PreflopNode> {
    let mut out = Vec::new();
    for seated in 6u8..=9 {
        for hero in positions_for(seated) {
            for scenario in scenarios_for(seated, hero) {
                for bucket in all_buckets() {
                    out.push(PreflopNode {
                        seated,
                        hero,
                        bucket,
                        scenario,
                    });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 各桌型的位置數正確() {
        for seated in 6u8..=9 {
            assert_eq!(
                positions_for(seated).len(),
                usize::from(seated),
                "{seated} 人桌應有 {seated} 個位置"
            );
        }
    }

    #[test]
    fn utg_不會面對更早位置的開牌() {
        let scenarios = scenarios_for(9, PositionLabel::Utg);
        assert!(
            !scenarios
                .iter()
                .any(|s| matches!(s, PreflopScenario::VsOpen { .. })),
            "UTG 是最早行動者，不可能面對開牌"
        );
        assert!(
            !scenarios
                .iter()
                .any(|s| matches!(s, PreflopScenario::VsLimp { .. })),
            "UTG 之前無人，不可能面對跛入"
        );
    }

    #[test]
    fn bb_不會被更晚位置_3bet() {
        let scenarios = scenarios_for(9, PositionLabel::Bb);
        assert!(
            !scenarios
                .iter()
                .any(|s| matches!(s, PreflopScenario::VsThreeBet { .. })),
            "BB 是最後行動者，其後無人可 3-bet"
        );
    }

    #[test]
    fn 節點鍵互異() {
        let nodes = enumerate_nodes();
        let mut keys: Vec<String> = nodes.iter().map(PreflopNode::key).collect();
        let before = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(before, keys.len(), "節點鍵必須互異，否則內容會互相覆蓋");
    }

    #[test]
    fn 每個節點都含九個_bucket() {
        let nodes = enumerate_nodes();
        assert_eq!(
            nodes.len() % all_buckets().len(),
            0,
            "節點總數應為 bucket 數的整數倍"
        );
    }
}
