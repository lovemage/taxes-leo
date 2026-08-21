//! 面對開牌（3-bet）的主動範圍寬度，**逐（桌型 × 英雄位置 × 開牌者位置）各自設定**。
//!
//! # 為什麼不用端點內插
//!
//! 與 `opening` 模組同一個病灶，但更嚴重。原本 `vs_open` 只給
//! `aggressive_earliest` 與 `aggressive_latest` 兩個端點，中間位置線性內插，
//! 而盲注位直接取 `aggressive_latest`。這造成兩個具體錯誤：
//!
//! 1. **`aggressive_earliest` 是死參數**。內插的終點落在最晚的非盲注位
//!    （BTN），盲注位又走另一條分支直接取端點，所以校準工作台上的兩個
//!    vsOpen 節點（BTN 面對 CO、BB 面對 BTN）算出來都是 `aggressive_latest`。
//!    顧問拖動「早位 3-bet 寬度」滑桿不會有任何一格改變。
//!
//! 2. **開牌者是誰完全不影響 3-bet 寬度**。舊模型只看英雄的位置，
//!    於是「BB 面對 UTG 開牌」與「BB 面對 BTN 開牌」被算成同一個寬度。
//!    這在撲克上明顯錯誤：UTG 開牌是強範圍，BB 該收緊；BTN 開牌多為偷盲，
//!    BB 該大幅放寬。`PreflopScenario::VsOpen` 早就把開牌者納入節點識別，
//!    寬度規則卻沒有讀它。
//!
//! 改為逐節點一個參數後，顧問調的每個數字都直接對應畫面上的一張表。
//!
//! # 預設值怎麼來的
//!
//! 兩個因素相乘：
//!
//! - **開牌者的開牌範圍寬度**。對手開得越寬，3-bet 越有利可圖，因此以
//!   `opening` 的預設寬度為基準取四分之一（對手開 43%，3-bet 約 10.8%）。
//! - **英雄身後仍須行動的人數**。身後人越多，被反擠壓的風險越高，
//!   3-bet 應收緊。
//!
//! 這兩個都是啟發式而非求解結果，是顧問校準的直接對象。

use std::collections::BTreeMap;

use crate::position::PositionLabel;
use crate::strategy::distribution::Myriad;
use crate::strategy::opening;
use crate::strategy::preflop::positions_for;

/// 逐（桌型 × 英雄位置 × 開牌者位置）的 3-bet 範圍寬度。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VsOpenWidths {
    values: BTreeMap<(u8, PositionLabel, PositionLabel), Myriad>,
}

impl VsOpenWidths {
    /// 工程佔位的預設值。
    ///
    /// 涵蓋 6～9 人桌全部合法的（英雄，開牌者）配對——開牌者必須在英雄之前，
    /// 與 [`crate::strategy::preflop::scenarios_for`] 的合法性判定一致。
    #[must_use]
    pub fn engineering_placeholder() -> Self {
        let mut values = BTreeMap::new();
        for seated in 6u8..=9 {
            let order = positions_for(seated);
            for (hero_index, hero) in order.iter().enumerate() {
                for opener in &order[..hero_index] {
                    values.insert(
                        (seated, *hero, *opener),
                        default_width(&order, hero_index, *opener),
                    );
                }
            }
        }
        Self { values }
    }

    /// 取某節點的 3-bet 寬度。未登錄時回傳依同一規則推得的預設值。
    #[must_use]
    pub fn get(&self, seated: u8, hero: PositionLabel, opener: PositionLabel) -> Myriad {
        if let Some(value) = self.values.get(&(seated, hero, opener)) {
            return *value;
        }
        let order = positions_for(seated);
        let hero_index = order.iter().position(|p| *p == hero).unwrap_or(0);
        default_width(&order, hero_index, opener)
    }

    /// 設定某節點的 3-bet 寬度。
    pub fn set(&mut self, seated: u8, hero: PositionLabel, opener: PositionLabel, value: Myriad) {
        self.values.insert((seated, hero, opener), value);
    }

    /// 全部已登錄的項目，供工作台展開成滑桿。
    #[must_use]
    pub fn entries(&self) -> Vec<((u8, PositionLabel, PositionLabel), Myriad)> {
        self.values.iter().map(|(k, v)| (*k, *v)).collect()
    }
}

/// 依開牌者的開牌範圍與英雄身後人數給出預設 3-bet 寬度（萬分比）。
///
/// 開牌者不在序列中（理論上不會發生）時視為身後 0 人，取最寬的基準。
#[must_use]
fn default_width(order: &[PositionLabel], hero_index: usize, opener: PositionLabel) -> Myriad {
    let opener_behind = order
        .iter()
        .position(|p| *p == opener)
        .map_or(0, |index| order.len() - 1 - index);
    let opener_width = opening::default_width(opener, opener_behind);

    // 對手開得越寬，3-bet 越有利可圖
    let base = opener_width / 4;

    // 身後每多一人被反擠壓的風險就高一分，3-bet 相應收緊。
    // 9-max 的 UTG+1 面對 UTG 身後尚有 7 人，是折扣最重的情形
    let behind = Myriad::try_from(order.len() - 1 - hero_index).unwrap_or(0);
    let discount = 10_000_u32.saturating_sub(behind * 400);

    base * discount / 10_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 涵蓋六到九人桌的全部合法配對() {
        let widths = VsOpenWidths::engineering_placeholder();
        // 每桌型的配對數為 n(n-1)/2：開牌者必須在英雄之前
        let expected: usize = (6..=9usize).map(|n| n * (n - 1) / 2).sum();
        assert_eq!(widths.entries().len(), expected, "6+7+8+9 人桌合計 100 個配對");
    }

    #[test]
    fn 開牌者是誰會改變三bet寬度() {
        let widths = VsOpenWidths::engineering_placeholder();
        let vs_utg = widths.get(9, PositionLabel::Bb, PositionLabel::Utg);
        let vs_btn = widths.get(9, PositionLabel::Bb, PositionLabel::Btn);
        assert!(
            vs_btn > vs_utg * 2,
            "BTN 開牌多為偷盲，BB 的 3-bet 應遠寬於面對 UTG（{vs_btn} vs {vs_utg}）"
        );
    }

    #[test]
    fn 身後人數越多三bet越緊() {
        let widths = VsOpenWidths::engineering_placeholder();
        // 同為面對 UTG 開牌，UTG+1 身後 7 人、BB 身後 0 人
        let early = widths.get(9, PositionLabel::Utg1, PositionLabel::Utg);
        let late = widths.get(9, PositionLabel::Bb, PositionLabel::Utg);
        assert!(early < late, "身後人多應收緊（{early} vs {late}）");
    }

    #[test]
    fn 可逐節點覆寫且不影響其他節點() {
        let mut widths = VsOpenWidths::engineering_placeholder();
        let before = widths.get(9, PositionLabel::Bb, PositionLabel::Btn);
        widths.set(9, PositionLabel::Btn, PositionLabel::Co, 2_000);

        assert_eq!(widths.get(9, PositionLabel::Btn, PositionLabel::Co), 2_000);
        assert_eq!(
            widths.get(9, PositionLabel::Bb, PositionLabel::Btn),
            before,
            "調整某節點不得連動其他節點——這正是改用逐節點參數的目的"
        );
    }

    #[test]
    fn 未登錄的配對回傳與登錄值相同的預設() {
        let widths = VsOpenWidths::engineering_placeholder();
        let order = positions_for(9);
        let hero_index = order
            .iter()
            .position(|p| *p == PositionLabel::Btn)
            .expect("BTN 在序列中");
        assert_eq!(
            widths.get(9, PositionLabel::Btn, PositionLabel::Co),
            default_width(&order, hero_index, PositionLabel::Co),
            "get 的 fallback 必須與建表時同一條規則"
        );
    }
}
