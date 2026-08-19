//! 比例指標的區間估計。
//!
//! 核心規格 5.3：「獲勝手數比例、VPIP、PFR 等比例顯示**分子／分母**；
//! 獨立近似用 Wilson interval，有 cluster 時對 cluster bootstrap。」
//!
//! 分子與分母必須一起顯示，因為 60% 在 3/5 與 600/1000 是完全不同的證據強度，
//! 只給百分比會讓兩者看起來一樣。

/// 比例估計。分子與分母一律保留，供 UI 依規格併同顯示。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Proportion {
    pub numerator: u64,
    pub denominator: u64,
    /// Wilson 區間下界（0.0～1.0）
    pub ci_low: f64,
    pub ci_high: f64,
}

impl Proportion {
    /// 點估計。分母為 0 時回傳 `None`——核心規格 5.4：
    /// 「所有比例顯示分子／分母與區間；**分母為 0 時顯示 N/A**」。
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn point(&self) -> Option<f64> {
        if self.denominator == 0 {
            return None;
        }
        Some(self.numerator as f64 / self.denominator as f64)
    }

    /// 區間半寬。分母為 0 時回傳 `None`。
    #[must_use]
    pub fn half_width(&self) -> Option<f64> {
        if self.denominator == 0 {
            return None;
        }
        Some((self.ci_high - self.ci_low) / 2.0)
    }
}

/// Wilson score interval（95%）。
///
/// 選 Wilson 而非常見的 normal approximation，是因為後者在比例接近 0 或 1、
/// 或樣本小時會給出超出 `[0,1]` 的荒謬區間；Wilson 恆落在合法範圍內。
/// 撲克的行為頻率（例如 3-bet）常態性地接近 0，正是 normal approximation
/// 最不可靠的區域。
///
/// # Panics
/// `numerator` 大於 `denominator` 時 panic：那代表計數邏輯有誤。
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn wilson(numerator: u64, denominator: u64) -> Proportion {
    assert!(numerator <= denominator, "分子不得大於分母");

    if denominator == 0 {
        return Proportion {
            numerator,
            denominator,
            ci_low: 0.0,
            ci_high: 0.0,
        };
    }

    const Z: f64 = 1.96;
    let n = denominator as f64;
    let p = numerator as f64 / n;
    let z2 = Z * Z;

    let denom = 1.0 + z2 / n;
    let centre = (p + z2 / (2.0 * n)) / denom;
    let spread = Z / denom * ((p * (1.0 - p) / n) + z2 / (4.0 * n * n)).sqrt();

    Proportion {
        numerator,
        denominator,
        ci_low: (centre - spread).max(0.0),
        ci_high: (centre + spread).min(1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 分母為零時點估計為_na() {
        let p = wilson(0, 0);
        assert_eq!(p.point(), None, "核心規格 5.4：分母為 0 顯示 N/A");
        assert_eq!(p.half_width(), None);
    }

    #[test]
    fn 區間恆落在合法範圍內() {
        for (k, n) in [(0, 10), (10, 10), (1, 1_000), (999, 1_000), (0, 1), (1, 1)] {
            let p = wilson(k, n);
            assert!(p.ci_low >= 0.0, "{k}/{n} 的下界不得為負");
            assert!(p.ci_high <= 1.0, "{k}/{n} 的上界不得超過 1");
            assert!(p.ci_low <= p.ci_high);
        }
    }

    #[test]
    fn 極端比例仍給出合理區間() {
        // normal approximation 在此處會給出負下界；Wilson 不會
        let p = wilson(0, 100);
        assert_eq!(p.point(), Some(0.0));
        assert!(p.ci_low >= 0.0);
        assert!(p.ci_high > 0.0, "0/100 的上界應為正，不能塌成 0");
        assert!(p.ci_high < 0.05, "0/100 的上界應接近 0");
    }

    #[test]
    fn 樣本越大區間越窄() {
        let small = wilson(3, 5);
        let large = wilson(600, 1_000);
        assert!(
            large.half_width().expect("有值") < small.half_width().expect("有值"),
            "600/1000 的區間必須遠窄於 3/5——這正是必須同時顯示分子分母的理由"
        );
    }

    #[test]
    fn 分子大於分母時_panic() {
        let result = std::panic::catch_unwind(|| wilson(11, 10));
        assert!(result.is_err(), "分子大於分母代表計數邏輯有誤，應立即停住");
    }

    #[test]
    fn 中間比例的區間包含真值() {
        let p = wilson(50, 100);
        assert!(p.ci_low < 0.5 && p.ci_high > 0.5);
    }
}
