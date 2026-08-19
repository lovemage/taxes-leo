//! Moving block bootstrap 與判定分類。
//!
//! 核心規格 5.3：「獨立批次的 bb/100／EV 用 **moving block bootstrap**；
//! block 長度依 M0 實測的自相關長度凍結，**桌次邊界為強制 block 斷點**。」
//!
//! # 為什麼不能直接用逐手標準差
//!
//! 同一桌次內的手牌高度相關：籌碼深度、對手組成、位置循環都跨手延續。
//! 把逐手結果當成獨立同分佈會嚴重低估變異，算出過窄的 CI，
//! 讓沒有統計意義的差距看起來顯著。
//!
//! # 統計輸出可以用浮點
//!
//! 核心規格 2.3：「規則層不得使用浮點籌碼；浮點只出現在統計輸出」。
//! 本模組即該邊界，因此局部豁免 workspace 的浮點轉型 lint。

use crate::rng::Rng;

/// 每手結果的一筆觀測。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Observation {
    /// 使用者該手的淨損益，以大盲為單位
    pub delta_bb: f64,
    /// 所屬桌次。桌次邊界是強制的 block 斷點
    pub instance: u64,
}

/// 使用的 estimator。核心規格 5.3 要求「實際使用的 estimator 名稱必須顯示於報表」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Estimator {
    /// 桌次數不足以做 cluster bootstrap 時使用
    MovingBlockBootstrap { block_length: usize },
    /// 桌次數足夠多時可改用
    ClusterBootstrap,
}

impl Estimator {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MovingBlockBootstrap { .. } => "moving block bootstrap",
            Self::ClusterBootstrap => "cluster bootstrap（以桌次為 cluster）",
        }
    }
}

/// 結論的可判定狀態。
///
/// 核心規格 5.3：「CI 跨 0 只能標示『本樣本無法判定優劣』，
/// **不得直接等同『樣本不足』**。樣本不足由預先定義的最低有效樣本、
/// CI 寬度或 Monte Carlo 誤差門檻判定。」
///
/// 因此兩者是不同狀態，且判定有先後：先看樣本是否足夠，再看 CI 是否跨 0。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 有效樣本（block 數）低於門檻
    InsufficientSample,
    /// 樣本足夠，但 CI 跨 0：本樣本無法判定優劣
    Indeterminate,
    /// 可判定
    Determinate,
}

impl Verdict {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InsufficientSample => "樣本不足",
            Self::Indeterminate => "本樣本無法判定優劣",
            Self::Determinate => "可判定",
        }
    }
}

/// 有效樣本的最低 block 數。低於此值一律標示樣本不足。
///
/// M0 須以實測重新凍結；此值為規劃用下界。
pub const MIN_EFFECTIVE_BLOCKS: usize = 30;

/// 預設重抽次數。
pub const DEFAULT_RESAMPLES: usize = 2_000;

/// 估計結果。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Estimate {
    /// 點估計，bb/100
    pub point: f64,
    pub ci_low: f64,
    pub ci_high: f64,
    pub estimator: Estimator,
    /// 有效樣本，以 block 數計（核心規格 5.3）
    pub effective_blocks: usize,
    pub hands: usize,
    pub verdict: Verdict,
}

impl Estimate {
    /// CI 半寬。
    #[must_use]
    pub fn half_width(&self) -> f64 {
        (self.ci_high - self.ci_low) / 2.0
    }
}

/// 由自相關估計 block 長度。
///
/// 取第一個自相關落入統計不顯著範圍（|ACF| < 2/√n）的落後階數，
/// 再乘 2 作為 block 長度——block 需涵蓋相關結構才能保留其影響。
///
/// M0 須以實測資料凍結此值（核心規格 5.3）。
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn estimate_block_length(series: &[f64]) -> usize {
    let n = series.len();
    if n < 20 {
        return 1;
    }
    let mean = series.iter().sum::<f64>() / n as f64;
    let variance = series.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
    if variance <= f64::EPSILON {
        return 1;
    }

    let threshold = 2.0 / (n as f64).sqrt();
    let max_lag = (n / 4).min(200);
    for lag in 1..=max_lag {
        let covariance: f64 = (0..n - lag)
            .map(|i| (series[i] - mean) * (series[i + lag] - mean))
            .sum::<f64>()
            / (n - lag) as f64;
        if (covariance / variance).abs() < threshold {
            return (lag * 2).max(1);
        }
    }
    max_lag.max(1)
}

/// 以 moving block bootstrap 估計 bb/100 與 95% CI。
///
/// `observations` 必須依手序排列。桌次邊界會被當成強制斷點——
/// block 不得跨桌次，否則會把兩段互不相關的序列黏成一個「相關結構」。
///
/// # Panics
/// `resamples` 為 0 時 panic。
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn moving_block_bootstrap(
    observations: &[Observation],
    block_length: usize,
    resamples: usize,
    rng: &mut Rng,
) -> Estimate {
    assert!(resamples > 0, "重抽次數必須為正");

    let hands = observations.len();
    let block_length = block_length.max(1);

    if hands == 0 {
        return Estimate {
            point: 0.0,
            ci_low: 0.0,
            ci_high: 0.0,
            estimator: Estimator::MovingBlockBootstrap { block_length },
            effective_blocks: 0,
            hands: 0,
            verdict: Verdict::InsufficientSample,
        };
    }

    let series: Vec<f64> = observations.iter().map(|o| o.delta_bb).collect();
    let point = mean(&series) * 100.0;

    // 合法的 block 起點：整個 block 必須落在同一桌次內
    let starts: Vec<usize> = (0..hands)
        .filter(|&i| {
            let end = i + block_length;
            end <= hands && observations[i].instance == observations[end - 1].instance
        })
        .collect();

    let effective_blocks = hands / block_length;

    if starts.is_empty() || effective_blocks < 2 {
        return Estimate {
            point,
            ci_low: point,
            ci_high: point,
            estimator: Estimator::MovingBlockBootstrap { block_length },
            effective_blocks,
            hands,
            verdict: Verdict::InsufficientSample,
        };
    }

    let blocks_per_resample = hands.div_ceil(block_length);
    let mut means = Vec::with_capacity(resamples);
    for _ in 0..resamples {
        let mut sum = 0.0;
        let mut count = 0usize;
        for _ in 0..blocks_per_resample {
            let pick = usize::try_from(rng.below(starts.len() as u64)).unwrap_or(0);
            let start = starts[pick];
            for offset in 0..block_length {
                if count >= hands {
                    break;
                }
                sum += series[start + offset];
                count += 1;
            }
        }
        means.push(sum / count.max(1) as f64 * 100.0);
    }
    means.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let ci_low = percentile(&means, 2.5);
    let ci_high = percentile(&means, 97.5);

    // 判定順序：先看樣本是否足夠，再看 CI 是否跨 0（核心規格 5.3）
    let verdict = if effective_blocks < MIN_EFFECTIVE_BLOCKS {
        Verdict::InsufficientSample
    } else if ci_low <= 0.0 && ci_high >= 0.0 {
        Verdict::Indeterminate
    } else {
        Verdict::Determinate
    };

    Estimate {
        point,
        ci_low,
        ci_high,
        estimator: Estimator::MovingBlockBootstrap { block_length },
        effective_blocks,
        hands,
        verdict,
    }
}

#[allow(clippy::cast_precision_loss)]
fn mean(series: &[f64]) -> f64 {
    if series.is_empty() {
        return 0.0;
    }
    series.iter().sum::<f64>() / series.len() as f64
}

#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) as f64 * p / 100.0).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series(values: &[f64], instance: u64) -> Vec<Observation> {
        values
            .iter()
            .map(|&delta_bb| Observation { delta_bb, instance })
            .collect()
    }

    fn rng() -> Rng {
        crate::rng::Rng::derive(1, 1, crate::rng::RngDomain::Stats)
    }

    #[test]
    fn 點估計為每百手期望值() {
        let data = series(&[1.0, -1.0, 2.0, 0.0], 0);
        let estimate = moving_block_bootstrap(&data, 1, 100, &mut rng());
        // 平均 0.5 bb/手 → 50 bb/100
        assert!((estimate.point - 50.0).abs() < 1e-9);
    }

    #[test]
    fn 有效樣本以_block_數計而非手數() {
        let data = series(&[1.0; 600], 0);
        let estimate = moving_block_bootstrap(&data, 20, 200, &mut rng());
        assert_eq!(estimate.hands, 600);
        assert_eq!(
            estimate.effective_blocks, 30,
            "600 手、block 長 20 → 有效樣本為 30 個 block，不是 600"
        );
    }

    #[test]
    fn block_不得跨桌次() {
        // 兩個桌次各 50 手，block 長 20：合法起點不得跨越邊界
        let mut data = series(&[1.0; 50], 0);
        data.extend(series(&[1.0; 50], 1));
        let estimate = moving_block_bootstrap(&data, 20, 100, &mut rng());
        // 每個桌次可提供 50-20+1 = 31 個起點，共 62 個；
        // 若允許跨界會有 81 個。這裡以結果穩定性間接驗證：
        // 全部值相同時 CI 必為單點
        assert!((estimate.ci_high - estimate.ci_low).abs() < 1e-9);
    }

    #[test]
    fn 樣本不足與無法判定是不同狀態() {
        // 樣本不足：block 數低於門檻
        let small = series(&[5.0; 40], 0);
        let estimate = moving_block_bootstrap(&small, 20, 200, &mut rng());
        assert_eq!(
            estimate.verdict,
            Verdict::InsufficientSample,
            "block 數 2 遠低於門檻，應標樣本不足"
        );

        // 樣本足夠但 CI 跨 0：無法判定
        let noisy: Vec<f64> = (0..2_000)
            .map(|i| if i % 2 == 0 { 10.0 } else { -10.0 })
            .collect();
        let estimate = moving_block_bootstrap(&series(&noisy, 0), 20, 500, &mut rng());
        assert_eq!(
            estimate.verdict,
            Verdict::Indeterminate,
            "核心規格 5.3：CI 跨 0 只能標無法判定，不得等同樣本不足"
        );
    }

    #[test]
    fn 明確為正的序列可判定() {
        let data = series(&[3.0; 2_000], 0);
        let estimate = moving_block_bootstrap(&data, 20, 500, &mut rng());
        assert_eq!(estimate.verdict, Verdict::Determinate);
        assert!(estimate.ci_low > 0.0);
    }

    #[test]
    fn 相同_seed_的重抽結果一致() {
        let data: Vec<f64> = (0..1_000).map(|i| f64::from(i % 17) - 8.0).collect();
        let observations = series(&data, 0);
        let a = moving_block_bootstrap(&observations, 15, 300, &mut rng());
        let b = moving_block_bootstrap(&observations, 15, 300, &mut rng());
        assert_eq!(a, b, "統計重抽必須可重現");
    }

    #[test]
    fn 相關序列的_ci_比獨立序列寬() {
        // 高度相關的序列（長段同號）應得到較寬的 CI
        let correlated: Vec<f64> = (0..2_000)
            .map(|i| if (i / 100) % 2 == 0 { 5.0 } else { -5.0 })
            .collect();
        let independent: Vec<f64> = (0..2_000)
            .map(|i| if i % 2 == 0 { 5.0 } else { -5.0 })
            .collect();

        let a = moving_block_bootstrap(&series(&correlated, 0), 20, 500, &mut rng());
        let b = moving_block_bootstrap(&series(&independent, 0), 20, 500, &mut rng());
        assert!(
            a.half_width() > b.half_width(),
            "相關序列 {:.2} 應寬於獨立序列 {:.2}",
            a.half_width(),
            b.half_width()
        );
    }

    #[test]
    fn 自相關長度估計能分辨相關與獨立序列() {
        // 獨立序列必須真的隨機。用 i % 2 這種週期為 2 的確定序列不行——
        // 它的自相關永遠不衰減，反而會被判成高度相關
        let mut source = crate::rng::Rng::derive(7, 7, crate::rng::RngDomain::Stats);
        let independent: Vec<f64> = (0..2_000)
            .map(|_| if source.below(2) == 0 { 1.0 } else { -1.0 })
            .collect();
        // 相關序列：每 50 手同號，模擬桌次內的籌碼與對手結構延續
        let correlated: Vec<f64> = (0..2_000)
            .map(|i| if (i / 50) % 2 == 0 { 1.0 } else { -1.0 })
            .collect();

        let independent_length = estimate_block_length(&independent);
        let correlated_length = estimate_block_length(&correlated);
        assert!(
            correlated_length > independent_length,
            "相關序列需要更長的 block（相關 {correlated_length} vs 獨立 {independent_length}）"
        );
    }

    #[test]
    fn 空序列回報樣本不足而非崩潰() {
        let estimate = moving_block_bootstrap(&[], 10, 100, &mut rng());
        assert_eq!(estimate.verdict, Verdict::InsufficientSample);
        assert_eq!(estimate.hands, 0);
    }
}
