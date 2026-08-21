//! 逐格覆寫：顧問說了、但參數表達不出來的那些格。
//!
//! # 為什麼需要這一層
//!
//! 校準的主線是**歸因**——顧問說「BTN 的 A5s 應該混合」，工具反解出
//! 「把 `opening[9max.BTN]` 從 43% 調到 45.8%」，並列出這一調會連帶
//! 拉進哪些手牌。走參數的好處是會自動泛化到其餘四千多個節點。
//!
//! 但有些意見**參數表達不出來**。例如「BTN 開 A5s 但不開 A6s」——
//! 兩者在 equity 排序上相鄰，任何門檻式參數都無法只收其中一個。
//! 這時只剩兩條路：
//!
//! 1. 記成逐格覆寫，並把它當作**模型缺口的清單**；
//! 2. 假裝顧問沒說過。
//!
//! 這個模組是第一條路。覆寫清單本身就是「模型還缺哪些參數」的證據，
//! 累積到一定數量就該回頭補參數，而不是無限長下去。
//!
//! # 覆寫不是預設路徑
//!
//! [`crate::strategy::calibration::attribute`] 應該先跑；只有在顧問看過
//! 歸因結果、明確表示「連帶影響不可接受」時才落到這裡。

use std::collections::BTreeMap;

use crate::strategy::distribution::{Myriad, FULL};
use crate::strategy::hand_class::HandClass;
use crate::strategy::preflop::PreflopNode;

/// 一格的覆寫值。
///
/// 只存主動與跟注兩個數字，棄牌是餘數——因此**不可能構造出合計不等於
/// 100% 的覆寫**，正規化錯誤在型別層就被排除。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverrideCell {
    aggressive: Myriad,
    call: Myriad,
}

impl OverrideCell {
    /// 建立覆寫。主動加跟注超過 100% 時回傳 `None`。
    #[must_use]
    pub const fn new(aggressive: Myriad, call: Myriad) -> Option<Self> {
        if aggressive + call > FULL {
            return None;
        }
        Some(Self { aggressive, call })
    }

    #[must_use]
    pub const fn aggressive(self) -> Myriad {
        self.aggressive
    }

    #[must_use]
    pub const fn call(self) -> Myriad {
        self.call
    }

    /// 棄牌頻率是餘數。
    #[must_use]
    pub const fn fold(self) -> Myriad {
        FULL - self.aggressive - self.call
    }
}

/// 逐（節點 × 牌類）的覆寫表。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CellOverrides {
    values: BTreeMap<(PreflopNode, HandClass), OverrideCell>,
}

impl CellOverrides {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 取某格的覆寫。沒有覆寫時回傳 `None`，產生器照常走參數。
    #[must_use]
    pub fn get(&self, node: &PreflopNode, class: HandClass) -> Option<OverrideCell> {
        self.values.get(&(*node, class)).copied()
    }

    /// 記下一格覆寫。
    pub fn set(&mut self, node: PreflopNode, class: HandClass, cell: OverrideCell) {
        self.values.insert((node, class), cell);
    }

    /// 移除一格覆寫，讓它回到參數產生的結果。
    pub fn clear(&mut self, node: &PreflopNode, class: HandClass) {
        self.values.remove(&(*node, class));
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// 全部覆寫，供匯出與「模型缺口清單」檢視。
    #[must_use]
    pub fn entries(&self) -> Vec<((PreflopNode, HandClass), OverrideCell)> {
        self.values.iter().map(|(k, v)| (*k, *v)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::PositionLabel;
    use crate::strategy::decision::StackBucket;
    use crate::strategy::preflop::PreflopScenario;

    fn node() -> PreflopNode {
        PreflopNode {
            seated: 9,
            hero: PositionLabel::Btn,
            bucket: StackBucket::VeryDeep,
            scenario: PreflopScenario::Unopened,
        }
    }

    fn class(label: &str) -> HandClass {
        HandClass::all()
            .into_iter()
            .find(|c| c.label() == label)
            .expect("牌類存在")
    }

    #[test]
    fn 棄牌是餘數因此恆正規化() {
        let cell = OverrideCell::new(5_000, 3_000).expect("合法");
        assert_eq!(cell.aggressive() + cell.call() + cell.fold(), FULL);
    }

    #[test]
    fn 合計超過百分之百的覆寫無法構造() {
        assert!(OverrideCell::new(6_000, 5_000).is_none());
        assert!(OverrideCell::new(FULL, 0).is_some());
    }

    #[test]
    fn 覆寫只影響指定的那一格() {
        let mut overrides = CellOverrides::new();
        overrides.set(node(), class("A5s"), OverrideCell::new(5_000, 0).expect("合法"));

        assert!(overrides.get(&node(), class("A5s")).is_some());
        assert!(
            overrides.get(&node(), class("A6s")).is_none(),
            "相鄰牌類不得被連帶覆寫"
        );

        let mut other = node();
        other.hero = PositionLabel::Co;
        assert!(
            overrides.get(&other, class("A5s")).is_none(),
            "其他節點的同一格不得被連帶覆寫"
        );
    }

    #[test]
    fn 清除後回到參數產生的結果() {
        let mut overrides = CellOverrides::new();
        overrides.set(node(), class("A5s"), OverrideCell::new(5_000, 0).expect("合法"));
        overrides.clear(&node(), class("A5s"));
        assert!(overrides.is_empty());
    }
}
