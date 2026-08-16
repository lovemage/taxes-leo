//! 桌型設定。
//!
//! 規格來源：核心規格 2.1、`德州撲克規則細則.md` 第二章。

use crate::chips::Chips;
use crate::pot::RakeConfig;

/// Ante 模式（核心規格 2.1，四種）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnteMode {
    #[default]
    None,
    /// 每名在座玩家各付
    PerPlayer,
    /// 由 BB 位一人代付全桌 ante 總額
    BbAnte,
    /// 由 BTN 位一人代付全桌 ante 總額
    BtnAnte,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AnteConfig {
    pub mode: AnteMode,
    /// 每人份的 ante 金額。`BbAnte`／`BtnAnte` 時代付者付
    /// 「金額 × 在座人數」（規則細則 2.2「全桌 ante 總額」）
    pub amount: Chips,
}

/// Straddle 設定（規則細則 2.3）。
///
/// `seats` 必須自 UTG 起連續，`amounts` 首段為 2×BB 且後段為前段的 2 倍。
/// 空的 `seats` 表示不使用 straddle。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StraddleConfig {
    pub seats: Vec<usize>,
    pub amounts: Vec<Chips>,
}

impl StraddleConfig {
    #[must_use]
    pub fn is_none(&self) -> bool {
        self.seats.is_empty()
    }

    /// 最大（最後一段）straddle 的座位與金額。
    #[must_use]
    pub fn largest(&self) -> Option<(usize, Chips)> {
        Some((*self.seats.last()?, *self.amounts.last()?))
    }

    /// 驗證是否符合規則細則 2.3 的金額遞增規則。
    ///
    /// # Errors
    /// 段數與金額數不符、首段非 2×BB，或後段非前段 2 倍時回傳錯誤。
    pub fn validate(&self, big_blind: Chips) -> Result<(), StraddleError> {
        if self.is_none() {
            return Ok(());
        }
        if self.seats.len() != self.amounts.len() {
            return Err(StraddleError::LengthMismatch);
        }
        let expected_first = Chips::new(big_blind.units() * 2);
        if self.amounts[0] != expected_first {
            return Err(StraddleError::FirstMustBeDoubleBigBlind {
                expected: expected_first,
                found: self.amounts[0],
            });
        }
        for pair in self.amounts.windows(2) {
            let expected = Chips::new(pair[0].units() * 2);
            if pair[1] != expected {
                return Err(StraddleError::MustDoublePrevious {
                    expected,
                    found: pair[1],
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StraddleError {
    LengthMismatch,
    FirstMustBeDoubleBigBlind { expected: Chips, found: Chips },
    MustDoublePrevious { expected: Chips, found: Chips },
}

/// 攤牌亮牌政策（規則細則 4.2，寫入 `RunManifest`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MuckPolicy {
    /// 依現實規則：輸家可 muck，只有實際亮出的底牌才是公開資訊
    #[default]
    Realistic,
    /// 全部攤牌亮牌（資訊最多，偏離現實）
    AlwaysShow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableConfig {
    pub small_blind: Chips,
    pub big_blind: Chips,
    pub ante: AnteConfig,
    pub straddle: StraddleConfig,
    pub rake: RakeConfig,
    pub muck: MuckPolicy,
}

impl TableConfig {
    /// 常用的無 ante、無 straddle、無抽水設定，供測試與基準情境使用。
    #[must_use]
    pub fn simple(small_blind: u64, big_blind: u64) -> Self {
        Self {
            small_blind: Chips::new(small_blind),
            big_blind: Chips::new(big_blind),
            ante: AnteConfig::default(),
            straddle: StraddleConfig::default(),
            rake: RakeConfig::NONE,
            muck: MuckPolicy::Realistic,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straddle_首段須為兩倍_bb_且後段須為前段兩倍() {
        let bb = Chips::new(2);

        let ok = StraddleConfig {
            seats: vec![2, 3],
            amounts: vec![Chips::new(4), Chips::new(8)],
        };
        assert_eq!(ok.validate(bb), Ok(()));

        // 規則細則 2.3：後段必須為前段的 2 倍，不是「大於前段」即可
        let bad = StraddleConfig {
            seats: vec![2, 3],
            amounts: vec![Chips::new(4), Chips::new(5)],
        };
        assert_eq!(
            bad.validate(bb),
            Err(StraddleError::MustDoublePrevious {
                expected: Chips::new(8),
                found: Chips::new(5),
            })
        );

        let bad_first = StraddleConfig {
            seats: vec![2],
            amounts: vec![Chips::new(3)],
        };
        assert!(matches!(
            bad_first.validate(bb),
            Err(StraddleError::FirstMustBeDoubleBigBlind { .. })
        ));
    }
}
