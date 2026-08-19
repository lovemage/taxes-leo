//! 可玩性調整：修正 raw equity 排序無法表達的翻後價值差異。
//!
//! # 為什麼需要這一層
//!
//! Equity 排序衡量的是「攤牌時誰的牌大」，但翻前決策的價值有一大部分來自
//! **翻後可玩性**，兩者並不等價。最明顯的證據來自校準工具的實測：
//! 要求收窄 `K2s` 時，最小連帶影響會一併排除 `87s`、`97s`、`T7s`——
//! 這三張的 equity 與 `K2s` 相近，但翻後價值差距極大，多數牌手不會接受
//! 這個交換。
//!
//! 這一層讓「同花連牌比弱同花高張更值得開」這件事可以被表達，
//! 而不必回到逐格覆寫。
//!
//! # 值是顧問的，機制是工程的
//!
//! 本模組提供**具名、受上下限約束的偏移**（比照核心規格 4.3 對 persona
//! 偏移的要求）。預設值刻意保守，它們是顧問的**首要調整對象**；
//! 工程這邊負責保證機制存在、可稽核、且不會被單一類別無限放大。

use crate::strategy::distribution::{Myriad, FULL};
use crate::strategy::hand_class::HandClass;

/// 可玩性類別。互斥且依固定順序判定，因此每個類別的調整效果可預測、
/// 可在報告中逐類說明。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlayabilityCategory {
    /// 口袋對子：set-mining 價值，raw equity 低估
    PocketPair,
    /// 同花 A：堅果同花與阻斷價值
    SuitedAce,
    /// 同花連牌（間隔 0，非 A）
    SuitedConnector,
    /// 同花一洞（間隔 1，非 A）
    SuitedOneGap,
    /// 同花兩洞（間隔 2，非 A）：順子潛力已明顯下降但仍存在
    SuitedTwoGap,
    /// 同花且間隔 3 以上（K2s、Q4s）：實質沒有順子潛力，且高張易被支配
    SuitedWideGap,
    /// 非同花 broadway（KJo、QTo）：易被支配
    OffsuitBroadway,
    /// 其餘非同花
    OffsuitOther,
}

impl PlayabilityCategory {
    /// 判定類別。順序即優先序，前面命中即回傳。
    #[must_use]
    pub fn of(class: HandClass) -> Self {
        if class.is_pair() {
            return Self::PocketPair;
        }
        if class.is_suited() {
            // 同花 A 優先於間隔判定：A5s 間隔雖大，但堅果同花與輪子價值
            // 使其行為更接近同花 A 而非弱同花高張
            if class.has_ace() {
                return Self::SuitedAce;
            }
            return match class.gap() {
                0 => Self::SuitedConnector,
                1 => Self::SuitedOneGap,
                2 => Self::SuitedTwoGap,
                _ => Self::SuitedWideGap,
            };
        }
        if class.is_broadway() {
            return Self::OffsuitBroadway;
        }
        Self::OffsuitOther
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PocketPair => "口袋對子",
            Self::SuitedAce => "同花 A",
            Self::SuitedConnector => "同花連牌",
            Self::SuitedOneGap => "同花一洞",
            Self::SuitedTwoGap => "同花兩洞",
            Self::SuitedWideGap => "同花大間隔",
            Self::OffsuitBroadway => "非同花 broadway",
            Self::OffsuitOther => "其餘非同花",
        }
    }

    pub const ALL: [Self; 8] = [
        Self::PocketPair,
        Self::SuitedAce,
        Self::SuitedConnector,
        Self::SuitedOneGap,
        Self::SuitedTwoGap,
        Self::SuitedWideGap,
        Self::OffsuitBroadway,
        Self::OffsuitOther,
    ];
}

/// 單一類別的偏移上限（萬分比）。
///
/// 設上限是為了防止某個類別的調整大到讓 equity 排序失去意義——
/// 若顧問需要超過上限的調整，代表該類別應再細分，而不是把旋鈕轉到底。
pub const MAX_SHIFT: i32 = 1_500;

/// 各可玩性類別的排序偏移（萬分比）。
///
/// 正值代表往「更強」的方向移動（百分位變小）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayabilityAdjustments {
    pub pocket_pair: i32,
    pub suited_ace: i32,
    pub suited_connector: i32,
    pub suited_one_gap: i32,
    pub suited_two_gap: i32,
    pub suited_wide_gap: i32,
    pub offsuit_broadway: i32,
    pub offsuit_other: i32,
}

impl PlayabilityAdjustments {
    /// 全部為 0：純 equity 排序，不做任何可玩性修正。
    ///
    /// 用於對照組——顧問可比較「有無調整」的差異，判斷調整是否合理。
    #[must_use]
    pub const fn none() -> Self {
        Self {
            pocket_pair: 0,
            suited_ace: 0,
            suited_connector: 0,
            suited_one_gap: 0,
            suited_two_gap: 0,
            suited_wide_gap: 0,
            offsuit_broadway: 0,
            offsuit_other: 0,
        }
    }

    /// 工程佔位的保守預設值。
    ///
    /// **這七個數字是顧問的首要調整對象。** 方向的依據是撲克的普遍共識
    /// （對子有 set-mining 價值、同花連牌有順花潛力、非同花大牌易被支配），
    /// 幅度則刻意保守——寧可讓顧問往上加，也不要一開始就過度修正而掩蓋
    /// equity 排序本身的訊號。
    #[must_use]
    pub const fn engineering_placeholder() -> Self {
        Self {
            pocket_pair: 500,
            suited_ace: 300,
            suited_connector: 700,
            suited_one_gap: 400,
            suited_two_gap: 150,
            suited_wide_gap: -500,
            offsuit_broadway: -200,
            offsuit_other: -500,
        }
    }

    #[must_use]
    pub const fn of(&self, category: PlayabilityCategory) -> i32 {
        match category {
            PlayabilityCategory::PocketPair => self.pocket_pair,
            PlayabilityCategory::SuitedAce => self.suited_ace,
            PlayabilityCategory::SuitedConnector => self.suited_connector,
            PlayabilityCategory::SuitedOneGap => self.suited_one_gap,
            PlayabilityCategory::SuitedTwoGap => self.suited_two_gap,
            PlayabilityCategory::SuitedWideGap => self.suited_wide_gap,
            PlayabilityCategory::OffsuitBroadway => self.offsuit_broadway,
            PlayabilityCategory::OffsuitOther => self.offsuit_other,
        }
    }

    /// 檢查所有偏移是否在上下限內。
    ///
    /// # Errors
    /// 任一類別超出 `±MAX_SHIFT` 時回傳該類別與其值。
    pub fn validate(&self) -> Result<(), (PlayabilityCategory, i32)> {
        for category in PlayabilityCategory::ALL {
            let value = self.of(category);
            if value.abs() > MAX_SHIFT {
                return Err((category, value));
            }
        }
        Ok(())
    }

    /// 套用偏移後的百分位。
    ///
    /// 結果 clamp 在 `[0, FULL]`，因此偏移不會讓任何牌跑出排序範圍。
    #[must_use]
    pub fn adjusted_percentile(&self, class: HandClass, base: Myriad) -> Myriad {
        let shift = self.of(PlayabilityCategory::of(class));
        let adjusted = i64::from(base) - i64::from(shift);
        Myriad::try_from(adjusted.clamp(0, i64::from(FULL))).unwrap_or(FULL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::Rank;
    use crate::strategy::ranking::class_of;

    #[test]
    fn 類別判定互斥且涵蓋全部_169_類() {
        let mut counts = [0usize; 8];
        for class in HandClass::all() {
            let category = PlayabilityCategory::of(class);
            let index = PlayabilityCategory::ALL
                .iter()
                .position(|c| *c == category)
                .expect("類別必在清單中");
            counts[index] += 1;
        }
        assert_eq!(counts.iter().sum::<usize>(), 169, "每類恰好歸入一個類別");
        assert_eq!(counts[0], 13, "13 個口袋對子");
    }

    #[test]
    fn 關鍵牌型的分類符合預期() {
        // 這是本模組存在的理由：K2s 與 87s 必須落在不同類別
        assert_eq!(
            PlayabilityCategory::of(class_of(Rank::King, Rank::Two, true)),
            PlayabilityCategory::SuitedWideGap
        );
        assert_eq!(
            PlayabilityCategory::of(class_of(Rank::Eight, Rank::Seven, true)),
            PlayabilityCategory::SuitedConnector
        );
        // A5s 走同花 A 而非大間隔：堅果同花與輪子價值
        assert_eq!(
            PlayabilityCategory::of(class_of(Rank::Ace, Rank::Five, true)),
            PlayabilityCategory::SuitedAce
        );
        assert_eq!(
            PlayabilityCategory::of(class_of(Rank::King, Rank::Jack, false)),
            PlayabilityCategory::OffsuitBroadway
        );
        assert_eq!(
            PlayabilityCategory::of(class_of(Rank::Seven, Rank::Two, false)),
            PlayabilityCategory::OffsuitOther
        );
    }

    /// T7s 與 K2s 不得同類。
    ///
    /// 初版把間隔 2 以上全歸為「大間隔」，導致 T7s（能做順子）與
    /// K2s（實質沒有順子潛力）共用同一懲罰。回歸測試在顧問看到前抓到。
    #[test]
    fn 兩洞同花與大間隔同花分屬不同類別() {
        let t7s = class_of(Rank::Ten, Rank::Seven, true);
        let k2s = class_of(Rank::King, Rank::Two, true);
        assert_eq!(PlayabilityCategory::of(t7s), PlayabilityCategory::SuitedTwoGap);
        assert_eq!(PlayabilityCategory::of(k2s), PlayabilityCategory::SuitedWideGap);

        let adjustments = PlayabilityAdjustments::engineering_placeholder();
        assert!(
            adjustments.adjusted_percentile(t7s, 4_100)
                < adjustments.adjusted_percentile(k2s, 4_100),
            "T7s 調整後必須強於 K2s"
        );
    }

    #[test]
    fn 零偏移不改變排序() {
        let none = PlayabilityAdjustments::none();
        for class in HandClass::all() {
            for base in [0, 1_234, 5_000, FULL] {
                assert_eq!(none.adjusted_percentile(class, base), base);
            }
        }
    }

    #[test]
    fn 偏移方向正確且不越界() {
        let adjustments = PlayabilityAdjustments::engineering_placeholder();
        let connector = class_of(Rank::Eight, Rank::Seven, true);
        let wide_gap = class_of(Rank::King, Rank::Two, true);

        // 同花連牌往強端移動（百分位變小）
        assert!(adjustments.adjusted_percentile(connector, 5_000) < 5_000);
        // 同花大間隔往弱端移動
        assert!(adjustments.adjusted_percentile(wide_gap, 5_000) > 5_000);

        // 邊界不越界
        assert_eq!(adjustments.adjusted_percentile(connector, 0), 0);
        assert_eq!(adjustments.adjusted_percentile(wide_gap, FULL), FULL);
    }

    #[test]
    fn 同花連牌調整後強於同花大間隔() {
        // 這正是校準工具指出的缺口：K2s 與 87s 的 equity 相近，
        // 調整後必須拉開
        let adjustments = PlayabilityAdjustments::engineering_placeholder();
        let base = 4_100; // 兩者實測的 equity 百分位相近
        let connector = adjustments.adjusted_percentile(class_of(Rank::Eight, Rank::Seven, true), base);
        let wide_gap = adjustments.adjusted_percentile(class_of(Rank::King, Rank::Two, true), base);
        assert!(
            connector < wide_gap,
            "87s（{connector}）調整後必須強於 K2s（{wide_gap}）"
        );
    }

    #[test]
    fn 預設值在上下限內() {
        assert_eq!(PlayabilityAdjustments::engineering_placeholder().validate(), Ok(()));
        assert_eq!(PlayabilityAdjustments::none().validate(), Ok(()));

        let excessive = PlayabilityAdjustments {
            suited_connector: MAX_SHIFT + 1,
            ..PlayabilityAdjustments::none()
        };
        assert!(
            excessive.validate().is_err(),
            "超出上限必須被攔下——需要更大調整代表該類別應再細分"
        );
    }
}
