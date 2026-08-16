//! 籌碼金額。
//!
//! 核心規格 2.3：所有牌局金額使用整數最小籌碼單位，規則層不得使用浮點籌碼。
//! 每手必須滿足「開始籌碼總額 = 結束籌碼總額 + 該手 rake，誤差為 0」，
//! 因此這裡刻意不提供 `f64` 轉換與除法；分池的整除與餘數由 `pot` 模組處理。

use std::fmt;
use std::ops::{Add, AddAssign, Sub, SubAssign};

/// 以最小籌碼單位計的金額，恆為非負。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct Chips(u64);

impl Chips {
    pub const ZERO: Self = Self(0);

    #[must_use]
    pub const fn new(units: u64) -> Self {
        Self(units)
    }

    #[must_use]
    pub const fn units(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// 飽和相減。用於「還需補多少才能跟注」這類語意上不該出現負值的計算。
    #[must_use]
    pub const fn saturating_sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }

    /// 取兩者較小值。用於有效籌碼與部分跟注。
    #[must_use]
    pub fn min_of(self, rhs: Self) -> Self {
        Self(self.0.min(rhs.0))
    }

    /// 依比例計算並向下取整至最小籌碼單位。
    ///
    /// 核心規格 2.1：rake「先依比例向下取整至最小籌碼單位，再套用每手 cap」。
    /// `basis_points` 為萬分比（例如 5% = 500），避免浮點進入規則層。
    #[must_use]
    pub const fn mul_basis_points_floor(self, basis_points: u32) -> Self {
        Self(self.0 * basis_points as u64 / 10_000)
    }
}

impl Add for Chips {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl AddAssign for Chips {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl Sub for Chips {
    type Output = Self;
    /// 溢位即 panic。規則層的減法若出現負值代表不變量已被破壞，
    /// 應當立刻停住而不是靜默產生錯誤金額。
    fn sub(self, rhs: Self) -> Self {
        Self(
            self.0
                .checked_sub(rhs.0)
                .expect("籌碼相減出現負值：不變量已被破壞"),
        )
    }
}

impl SubAssign for Chips {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl std::iter::Sum for Chips {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, Add::add)
    }
}

impl fmt::Display for Chips {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 相減出現負值時_panic() {
        let r = std::panic::catch_unwind(|| Chips::new(3) - Chips::new(5));
        assert!(r.is_err(), "負值相減必須 panic，不得靜默 wrap");
    }

    #[test]
    fn 比例計算向下取整() {
        // 5% of 199 = 9.95 → 9
        assert_eq!(Chips::new(199).mul_basis_points_floor(500), Chips::new(9));
        // 5% of 200 = 10
        assert_eq!(Chips::new(200).mul_basis_points_floor(500), Chips::new(10));
    }
}
