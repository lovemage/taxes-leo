//! 開牌範圍寬度，**逐（桌型 × 位置）各自設定**。
//!
//! # 為什麼不用內插
//!
//! 初版只給最早與最晚兩個端點，中間位置以線性內插產生。牌手顧問於
//! 2026-08-19 指出「左邊有 9 個位置，右邊也該有 9 個」——這不只是介面
//! 對應的問題，而是模型錯誤：**真實的開牌範圍不是線性的**。
//!
//! 從 UTG 到 UTG+1 只寬一點點，CO 到 BTN 卻是一大跳。線性內插在 9-max
//! 會把 UTG+1 算成 17.7%，實際應在 13% 附近。中間位置因此全部偏寬。
//!
//! 改為逐位置各一個參數後，顧問調的每個數字都直接對應畫面上的一張表，
//! 也不再有內插造成的系統性偏差。
//!
//! # 預設值怎麼來的
//!
//! 依**身後仍須行動的人數**給定。這是開牌範圍寬度的主要驅動因素，
//! 也讓不同桌型的預設值自動一致：6-max 的 UTG 身後有 5 人，
//! 與 9-max 的 LJ 相同，兩者的預設寬度因此相同。
//!
//! 盲注位不套用這個規則。SB 身後只有 1 人卻是翻後最不利位置，
//! 若照人數給就會比 BTN 還寬。

use std::collections::BTreeMap;

use crate::position::PositionLabel;
use crate::strategy::distribution::Myriad;
use crate::strategy::preflop::positions_for;

/// 逐（桌型 × 位置）的開牌範圍寬度。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningWidths {
    values: BTreeMap<(u8, PositionLabel), Myriad>,
}

impl OpeningWidths {
    /// 工程佔位的預設值。
    ///
    /// 這 30 個數字（6～9 人桌的全部位置）是顧問的直接調整對象。
    #[must_use]
    pub fn engineering_placeholder() -> Self {
        let mut values = BTreeMap::new();
        for seated in 6u8..=9 {
            let order = positions_for(seated);
            for (index, position) in order.iter().enumerate() {
                let behind = order.len() - 1 - index;
                values.insert((seated, *position), default_width(*position, behind));
            }
        }
        Self { values }
    }

    /// 取某桌型某位置的寬度。未登錄時回傳依人數推得的預設值。
    #[must_use]
    pub fn get(&self, seated: u8, position: PositionLabel) -> Myriad {
        if let Some(value) = self.values.get(&(seated, position)) {
            return *value;
        }
        let order = positions_for(seated);
        let behind = order
            .iter()
            .position(|p| *p == position)
            .map_or(0, |index| order.len() - 1 - index);
        default_width(position, behind)
    }

    /// 設定某桌型某位置的寬度。
    pub fn set(&mut self, seated: u8, position: PositionLabel, value: Myriad) {
        self.values.insert((seated, position), value);
    }

    /// 全部已登錄的項目，供工作台展開成滑桿。
    #[must_use]
    pub fn entries(&self) -> Vec<((u8, PositionLabel), Myriad)> {
        self.values.iter().map(|(k, v)| (*k, *v)).collect()
    }
}

/// 依位置與身後人數給出預設寬度（萬分比）。
///
/// 非盲注位隨身後人數遞減；盲注位另行給值，因為它們翻後不利，
/// 不能照「身後人數少就該寬」推。
#[must_use]
pub(crate) fn default_width(position: PositionLabel, players_behind: usize) -> Myriad {
    match position {
        // SB 身後只有 1 人，但翻後最不利，因此窄於 BTN
        PositionLabel::Sb => 3_800,
        // BB 在無人開牌時本就能過牌，主動範圍最窄
        PositionLabel::Bb => 2_000,
        _ => match players_behind {
            0 | 1 => 4_300,
            2 => 4_300, // BTN
            3 => 2_800, // CO
            4 => 2_200, // HJ
            5 => 1_800, // LJ（6-max 的 UTG 亦同）
            6 => 1_550, // UTG+2
            7 => 1_350, // UTG+1
            _ => 1_200, // UTG（9-max）
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 涵蓋六到九人桌的全部位置() {
        let widths = OpeningWidths::engineering_placeholder();
        let expected: usize = (6..=9usize).sum();
        assert_eq!(
            widths.entries().len(),
            expected,
            "6+7+8+9 = 30 個位置槽都要有值"
        );
    }

    #[test]
    fn 九人桌的寬度隨位置遞增且非線性() {
        let widths = OpeningWidths::engineering_placeholder();
        let order = positions_for(9);
        let non_blind: Vec<Myriad> = order
            .iter()
            .filter(|p| !matches!(p, PositionLabel::Sb | PositionLabel::Bb))
            .map(|p| widths.get(9, *p))
            .collect();

        for pair in non_blind.windows(2) {
            assert!(pair[1] > pair[0], "非盲注位的寬度必須遞增");
        }

        // 關鍵：增幅必須加速，不能等距。線性內插正是初版的錯誤
        let early_step = non_blind[1] - non_blind[0];
        let late_step = non_blind[non_blind.len() - 1] - non_blind[non_blind.len() - 2];
        assert!(
            late_step > early_step * 3,
            "CO 到 BTN 的增幅（{late_step}）應遠大於 UTG 到 UTG+1（{early_step}）"
        );
    }

    #[test]
    fn 盲注位不寬於按鈕位() {
        let widths = OpeningWidths::engineering_placeholder();
        let button = widths.get(9, PositionLabel::Btn);
        assert!(widths.get(9, PositionLabel::Sb) < button);
        assert!(widths.get(9, PositionLabel::Bb) < button);
    }

    #[test]
    fn 身後人數相同的位置預設值一致() {
        let widths = OpeningWidths::engineering_placeholder();
        // 6-max 的 UTG 身後 5 人，與 9-max 的 LJ 相同
        assert_eq!(
            widths.get(6, PositionLabel::Utg),
            widths.get(9, PositionLabel::Lj),
            "身後人數是寬度的主要驅動因素，相同人數應給相同預設值"
        );
    }

    #[test]
    fn 可逐位置覆寫且不影響其他位置() {
        let mut widths = OpeningWidths::engineering_placeholder();
        let before_utg1 = widths.get(9, PositionLabel::Utg1);
        widths.set(9, PositionLabel::Utg, 900);

        assert_eq!(widths.get(9, PositionLabel::Utg), 900);
        assert_eq!(
            widths.get(9, PositionLabel::Utg1),
            before_utg1,
            "調整某位置不得連動其他位置——這正是改用逐位置參數的目的"
        );
    }
}
