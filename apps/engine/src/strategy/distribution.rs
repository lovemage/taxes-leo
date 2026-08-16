//! 行動頻率分佈。
//!
//! 核心規格 4.1：「每個 hand class 在移除不合法行動後，Fold／Call／
//! Raise(size)／All-in 等頻率合計必須為 **100%**。」
//! 4.2：「不合法行動先 mask，再正規化；若剩餘權重為 0，必須進入 fallback，
//! **不得除以 0 或任選行動**。」
//!
//! 頻率以**萬分比整數**表示而非浮點。理由與籌碼相同（核心規格 2.3 的精神）：
//! 「合計等於 100%」如果用浮點就只能靠容差判斷，用整數則是精確相等，
//! 驗證與正規化都不會有累積誤差。

use crate::betting::Action;

/// 萬分比：10000 = 100%。
pub type Myriad = u32;

/// 合計必須等於此值。
pub const FULL: Myriad = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistributionError {
    /// 頻率合計不等於 100%
    NotNormalised { total: Myriad },
    /// mask 後所有合法行動的權重皆為 0，必須進入 fallback
    AllWeightsMasked,
    Empty,
}

/// 一個決策節點上的行動頻率分佈。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDistribution {
    entries: Vec<(Action, Myriad)>,
}

impl ActionDistribution {
    /// 建立分佈並驗證合計為 100%。
    ///
    /// # Errors
    /// 合計不等於 `FULL` 或項目為空時回傳錯誤。
    pub fn new(entries: Vec<(Action, Myriad)>) -> Result<Self, DistributionError> {
        if entries.is_empty() {
            return Err(DistributionError::Empty);
        }
        let total: Myriad = entries.iter().map(|(_, w)| *w).sum();
        if total != FULL {
            return Err(DistributionError::NotNormalised { total });
        }
        Ok(Self { entries })
    }

    /// 由未正規化的權重建立，自動正規化為合計 100%。
    ///
    /// 餘數依權重由大到小逐一分配，確保合計精確等於 `FULL` 且結果確定
    /// （相同輸入恆得相同輸出，不受 HashMap 迭代順序之類的影響）。
    ///
    /// # Errors
    /// 權重全為 0 或項目為空時回傳錯誤——此時必須進入 fallback，
    /// 不得任選行動（核心規格 4.2）。
    pub fn from_weights(weights: Vec<(Action, u64)>) -> Result<Self, DistributionError> {
        if weights.is_empty() {
            return Err(DistributionError::Empty);
        }
        let total: u64 = weights.iter().map(|(_, w)| *w).sum();
        if total == 0 {
            return Err(DistributionError::AllWeightsMasked);
        }

        let mut entries: Vec<(Action, Myriad)> = weights
            .iter()
            .map(|&(action, weight)| {
                let share = weight * u64::from(FULL) / total;
                (action, Myriad::try_from(share).unwrap_or(FULL))
            })
            .collect();

        // 分配整除後的餘數。依「捨去的小數部分」由大到小給，
        // 同分時依原順序，結果因此完全確定
        let assigned: Myriad = entries.iter().map(|(_, w)| *w).sum();
        let mut remainder = FULL - assigned;
        if remainder > 0 {
            let mut order: Vec<usize> = (0..weights.len()).collect();
            order.sort_by_key(|&i| {
                let exact = weights[i].1 * u64::from(FULL);
                std::cmp::Reverse(exact % total)
            });
            for &i in &order {
                if remainder == 0 {
                    break;
                }
                entries[i].1 += 1;
                remainder -= 1;
            }
        }

        Self::new(entries)
    }

    /// 移除不合法行動後重新正規化。
    ///
    /// 核心規格 4.2 的固定順序：**先 mask，再正規化**。
    ///
    /// # Errors
    /// 剩餘權重為 0 時回傳 [`DistributionError::AllWeightsMasked`]，
    /// 呼叫端必須改走 fallback。
    pub fn mask_and_renormalise(
        &self,
        is_legal: impl Fn(Action) -> bool,
    ) -> Result<Self, DistributionError> {
        let kept: Vec<(Action, u64)> = self
            .entries
            .iter()
            .filter(|(action, _)| is_legal(*action))
            .map(|&(action, weight)| (action, u64::from(weight)))
            .collect();

        // 遮蔽後沒有任何合法行動，與「合法行動的權重全為 0」是同一種
        // 情況：必須進入 fallback。回報同一個錯誤，讓呼叫端不必分辨兩者
        if kept.is_empty() {
            return Err(DistributionError::AllWeightsMasked);
        }
        Self::from_weights(kept)
    }

    #[must_use]
    pub fn entries(&self) -> &[(Action, Myriad)] {
        &self.entries
    }

    #[must_use]
    pub fn weight_of(&self, action: Action) -> Myriad {
        self.entries
            .iter()
            .find(|(a, _)| *a == action)
            .map_or(0, |(_, w)| *w)
    }

    /// 以給定的亂數值取樣。
    ///
    /// `roll` 必須落在 `[0, FULL)`。取樣依項目順序累加，因此相同的
    /// 分佈與 roll 恆得相同行動（核心規格 3.4 的可重現要求）。
    #[must_use]
    pub fn sample(&self, roll: Myriad) -> Action {
        let mut cumulative = 0;
        for &(action, weight) in &self.entries {
            cumulative += weight;
            if roll < cumulative {
                return action;
            }
        }
        // 浮點沒有介入，合計恆為 FULL，理論上到不了這裡；
        // 保底回傳最後一項而非 panic，避免取樣成為執行期風險
        self.entries
            .last()
            .map_or(Action::Fold, |&(action, _)| action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chips::Chips;

    #[test]
    fn 合計不為百分之百時拒絕建立() {
        let result = ActionDistribution::new(vec![(Action::Fold, 5000), (Action::Call, 4000)]);
        assert_eq!(
            result,
            Err(DistributionError::NotNormalised { total: 9000 })
        );
    }

    #[test]
    fn 正規化後合計精確等於百分之百() {
        // 三等分無法整除，餘數必須被分配掉
        let d = ActionDistribution::from_weights(vec![
            (Action::Fold, 1),
            (Action::Call, 1),
            (Action::AllIn, 1),
        ])
        .expect("正規化");
        let total: Myriad = d.entries().iter().map(|(_, w)| *w).sum();
        assert_eq!(total, FULL, "合計必須精確等於 10000，不接受容差");
    }

    #[test]
    fn 正規化結果是確定的() {
        let weights = vec![(Action::Fold, 7), (Action::Call, 11), (Action::AllIn, 3)];
        let a = ActionDistribution::from_weights(weights.clone()).expect("正規化");
        let b = ActionDistribution::from_weights(weights).expect("正規化");
        assert_eq!(a, b, "相同輸入必須得到相同輸出");
    }

    #[test]
    fn mask_後重新正規化且順序為先遮後正規化() {
        let d = ActionDistribution::new(vec![
            (Action::Fold, 3000),
            (Action::Call, 3000),
            (Action::RaiseTo(Chips::new(10)), 4000),
        ])
        .expect("建立");

        // 移除加注後，剩下 fold/call 各 3000 → 應正規化為各 5000
        let masked = d
            .mask_and_renormalise(|a| !matches!(a, Action::RaiseTo(_)))
            .expect("遮蔽後仍有合法行動");
        assert_eq!(masked.weight_of(Action::Fold), 5000);
        assert_eq!(masked.weight_of(Action::Call), 5000);
        assert_eq!(masked.weight_of(Action::RaiseTo(Chips::new(10))), 0);
    }

    #[test]
    fn 剩餘權重為零時必須回報而非任選行動() {
        let d = ActionDistribution::new(vec![(Action::RaiseTo(Chips::new(10)), 10_000)])
            .expect("建立");
        let result = d.mask_and_renormalise(|a| !matches!(a, Action::RaiseTo(_)));
        assert_eq!(
            result,
            Err(DistributionError::AllWeightsMasked),
            "核心規格 4.2：剩餘權重為 0 必須進入 fallback，不得除以 0 或任選"
        );
    }

    #[test]
    fn 取樣涵蓋每個非零項且可重現() {
        let d = ActionDistribution::new(vec![
            (Action::Fold, 2500),
            (Action::Call, 2500),
            (Action::AllIn, 5000),
        ])
        .expect("建立");

        assert_eq!(d.sample(0), Action::Fold);
        assert_eq!(d.sample(2499), Action::Fold);
        assert_eq!(d.sample(2500), Action::Call);
        assert_eq!(d.sample(4999), Action::Call);
        assert_eq!(d.sample(5000), Action::AllIn);
        assert_eq!(d.sample(9999), Action::AllIn);
    }

    #[test]
    fn 零權重項目不會被取樣到() {
        let d = ActionDistribution::new(vec![
            (Action::Fold, 0),
            (Action::Call, 10_000),
        ])
        .expect("建立");
        for roll in [0, 1, 5000, 9999] {
            assert_eq!(d.sample(roll), Action::Call, "權重為 0 的行動不得被取樣");
        }
    }
}
