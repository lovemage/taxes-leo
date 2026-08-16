//! 決策視角：策略層唯一能看到的資訊。
//!
//! 核心規格 2.4（不可放寬）：
//! - 「引擎內部的完整狀態與決策提供者取得的資訊必須使用**不同型別**。」
//! - 「`DecisionView` 只包含該座位可見的底牌、公共牌、公開行動、公開籌碼、
//!   位置、底池與策略所需的推導特徵；**不得包含牌堆順序、其他未亮底牌或
//!   未公開結果**。」
//! - 「`StrategyProvider` 的唯一輸入是 `DecisionView`、自身 range 與由公開
//!   資訊推導的對手 range estimates，**不得接收完整 `GameState`**。」
//!
//! 這裡的做法是讓隱藏資訊**在型別上不存在**：本結構沒有任何欄位能承載
//! 他人底牌或牌堆，因此不是「策略層有紀律地不去讀」，而是根本讀不到。
//! 建構子只從公開資訊組裝，是唯一的入口。

use crate::betting::{Action, LegalActions};
use crate::card::Card;
use crate::chips::Chips;
use crate::hand::Street;
use crate::position::PositionLabel;
use crate::strategy::hand_class::HandClass;

/// 有效籌碼分檔（規則細則 8.5）。
///
/// 邊界採左閉右開，避免邊界值同時落入兩桶。實際邊界於 M0 依實測凍結，
/// 這裡是規格列出的建議值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StackBucket {
    /// [0,15)
    VeryShort,
    /// [15,25)
    Short,
    /// [25,40)
    Medium,
    /// [40,70)
    Deep,
    /// [70,110)
    Deeper,
    /// [110,160)
    Deepest,
    /// [160,240)
    VeryDeep,
    /// [240,400)
    UltraDeep,
    /// [400,∞)
    Unbounded,
}

impl StackBucket {
    /// 由有效籌碼的大盲數判定。
    ///
    /// 規則細則 8.5：「有效籌碼以英雄與該節點相關對手之中的較小者計算，
    /// 並在四捨五入前先換算為 BB 的定點值；bucket 判定使用該定點值，
    /// 不得先取整再判定。」因此這裡收的是百分之一 BB 的定點值。
    #[must_use]
    pub const fn from_centi_bb(centi_bb: u64) -> Self {
        match centi_bb {
            0..=1_499 => Self::VeryShort,
            1_500..=2_499 => Self::Short,
            2_500..=3_999 => Self::Medium,
            4_000..=6_999 => Self::Deep,
            7_000..=10_999 => Self::Deeper,
            11_000..=15_999 => Self::Deepest,
            16_000..=23_999 => Self::VeryDeep,
            24_000..=39_999 => Self::UltraDeep,
            _ => Self::Unbounded,
        }
    }

    /// 策略內容的欄位鍵。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VeryShort => "0-15",
            Self::Short => "15-25",
            Self::Medium => "25-40",
            Self::Deep => "40-70",
            Self::Deeper => "70-110",
            Self::Deepest => "110-160",
            Self::VeryDeep => "160-240",
            Self::UltraDeep => "240-400",
            Self::Unbounded => "400+",
        }
    }
}

/// 一筆公開行動。只含依規則已公開的資訊。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicAction {
    pub street: Street,
    pub seat: usize,
    pub position: PositionLabel,
    pub action: Action,
}

/// 對手的公開狀態。**刻意沒有底牌欄位。**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpponentPublic {
    pub seat: usize,
    pub position: PositionLabel,
    /// 公開籌碼
    pub stack: Chips,
    /// 本街已投入
    pub committed: Chips,
    pub folded: bool,
    pub all_in: bool,
}

/// 策略層可見的一切。
///
/// **本結構沒有任何欄位能承載他人底牌或牌堆順序**，這是核心規格 2.4 的
/// 型別層落實。新增欄位時必須確認該資訊依規則已公開。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionView {
    pub seat: usize,
    pub position: PositionLabel,
    pub street: Street,
    /// 自己的底牌。看得到自己的牌是合法的
    pub hole_cards: [Card; 2],
    /// 已發出的公共牌
    pub board: Vec<Card>,
    pub seated: usize,
    pub effective_stack_bucket: StackBucket,
    pub pot: Chips,
    pub to_call: Chips,
    /// 由引擎產生的合法行動。策略層不得自行推導（核心規格 2.2）
    pub legal: LegalActions,
    /// 完整的公開行動歷史（核心規格 4.1 的 node 要素）
    pub history: Vec<PublicAction>,
    pub opponents: Vec<OpponentPublic>,
}

impl DecisionView {
    /// 自己底牌的 169 類歸屬，翻前策略的索引鍵。
    #[must_use]
    pub fn hand_class(&self) -> HandClass {
        HandClass::from_cards(self.hole_cards[0], self.hole_cards[1])
    }

    /// 本街仍在牌局的人數。
    #[must_use]
    pub fn active_opponents(&self) -> usize {
        self.opponents.iter().filter(|o| !o.folded).count()
    }

    /// 只保留本街的公開行動。
    #[must_use]
    pub fn current_street_history(&self) -> Vec<&PublicAction> {
        self.history
            .iter()
            .filter(|a| a.street == self.street)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 分檔邊界為左閉右開() {
        // 邊界值必須落在後一桶，不得同時屬於兩桶
        assert_eq!(StackBucket::from_centi_bb(1_499), StackBucket::VeryShort);
        assert_eq!(StackBucket::from_centi_bb(1_500), StackBucket::Short);
        assert_eq!(StackBucket::from_centi_bb(2_499), StackBucket::Short);
        assert_eq!(StackBucket::from_centi_bb(2_500), StackBucket::Medium);
        // 200BB 起始深度應落在 [160,240)
        assert_eq!(StackBucket::from_centi_bb(20_000), StackBucket::VeryDeep);
        assert_eq!(StackBucket::from_centi_bb(40_000), StackBucket::Unbounded);
    }

    #[test]
    fn 九個分檔的鍵互異() {
        let buckets = [
            StackBucket::VeryShort,
            StackBucket::Short,
            StackBucket::Medium,
            StackBucket::Deep,
            StackBucket::Deeper,
            StackBucket::Deepest,
            StackBucket::VeryDeep,
            StackBucket::UltraDeep,
            StackBucket::Unbounded,
        ];
        let mut keys: Vec<&str> = buckets.iter().map(|b| b.as_str()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), 9, "規則細則 8.5 的九檔，鍵必須互異");
    }

    #[test]
    fn 不取整再判定() {
        // 14.99BB 屬 [0,15) 而非先取整成 15 後落入 [15,25)
        assert_eq!(StackBucket::from_centi_bb(1_499), StackBucket::VeryShort);
    }
}
