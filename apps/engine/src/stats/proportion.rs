//! 比例指標的區間估計。
//!
//! 核心規格 5.3：「獲勝手數比例、VPIP、PFR 等比例顯示**分子／分母**；
//! 獨立近似用 Wilson interval，有 cluster 時對 cluster bootstrap。」
//!
//! 分子與分母必須一起顯示，因為 60% 在 3/5 與 600/1000 是完全不同的證據強度，
//! 只給百分比會讓兩者看起來一樣。
//!
//! # 為什麼比例也需要 cluster
//!
//! 同一桌次內的手牌不是獨立樣本：籌碼深度、對手組成與位置循環都跨手延續，
//! 使用者在某一桌打得緊，那一整段的 VPIP 會一起偏低。把 600 手當成 600 筆
//! 獨立觀測，Wilson 區間會窄到與證據不相稱——分母看起來很大，實際上只有
//! 幾個桌次在講話。因此桌次數足夠時改以桌次為 cluster 重抽，
//! 不足時退回 Wilson 並**如實標示是獨立近似**（核心規格 5.3、UI 規格 F.3.1）。

use crate::rng::Rng;

use super::bootstrap::percentile;

/// 使用的比例 estimator。
///
/// 核心規格 5.3：「實際使用的 estimator 名稱必須顯示於報表」。比例與
/// bb/100 各有自己的方法，因此不共用 [`super::Estimator`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProportionEstimator {
    /// Wilson score interval。把每一筆觀測當成獨立樣本
    WilsonIndependent,
    /// 以桌次為 cluster 的成對重抽
    ClusterBootstrap,
}

impl ProportionEstimator {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WilsonIndependent => "Wilson interval（獨立近似）",
            Self::ClusterBootstrap => "cluster bootstrap（以桌次為 cluster）",
        }
    }
}

/// 一個 cluster（桌次）內的分子與分母。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClusterCount {
    pub numerator: u64,
    pub denominator: u64,
}

/// 改用 cluster bootstrap 所需的最低 cluster 數。
///
/// 低於此值時重抽本身也不可靠——十個桌次的重抽只是在十個值之間打轉，
/// 會給出比 Wilson 更沒有根據的區間。因此退回 Wilson，並由
/// [`Proportion::estimator`] 如實告訴使用者用的是獨立近似。
///
/// 此值為規劃用下界，M0 須以實測重新凍結。
pub const MIN_CLUSTERS_FOR_BOOTSTRAP: usize = 20;

/// 比例重抽的預設次數。
pub const DEFAULT_PROPORTION_RESAMPLES: usize = 1_000;

/// 比例估計。分子與分母一律保留，供 UI 依規格併同顯示。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Proportion {
    pub numerator: u64,
    pub denominator: u64,
    /// 95% 區間下界（0.0～1.0）
    pub ci_low: f64,
    pub ci_high: f64,
    /// 實際使用的方法（核心規格 5.3）
    pub estimator: ProportionEstimator,
    /// 有效樣本：有分母的 cluster（桌次）數。
    ///
    /// 由 [`wilson`] 直接產生時為 0——那條路徑根本沒有看到 cluster 結構。
    /// [`cluster_bootstrap`] 退回 Wilson 時仍會填入實際 cluster 數，
    /// 使用者才知道區間為什麼是近似的
    pub effective_clusters: usize,
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
            estimator: ProportionEstimator::WilsonIndependent,
            effective_clusters: 0,
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
        estimator: ProportionEstimator::WilsonIndependent,
        effective_clusters: 0,
    }
}

/// 以桌次為 cluster 的成對重抽（95%）。
///
/// 每次重抽有放回地抽出與原本一樣多的桌次，把各桌次的分子與分母**成對**
/// 加總後才相除——分子分母分開重抽會拆散同一桌次內的相依，等於又回到
/// 獨立假設。
///
/// cluster 數低於 [`MIN_CLUSTERS_FOR_BOOTSTRAP`] 時退回 [`wilson`]，並在
/// `estimator` 如實標示；[`Proportion::effective_clusters`] 仍填入實際
/// cluster 數，讓 UI 說得出「為什麼這是近似值」。
///
/// # Panics
/// `resamples` 為 0，或分子合計大於分母合計時 panic。
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn cluster_bootstrap(clusters: &[ClusterCount], resamples: usize, rng: &mut Rng) -> Proportion {
    assert!(resamples > 0, "重抽次數必須為正");

    // 沒有分母的桌次不是樣本，留著只會稀釋重抽
    let contributing: Vec<ClusterCount> = clusters
        .iter()
        .copied()
        .filter(|c| c.denominator > 0)
        .collect();
    let numerator: u64 = contributing.iter().map(|c| c.numerator).sum();
    let denominator: u64 = contributing.iter().map(|c| c.denominator).sum();
    let effective_clusters = contributing.len();

    if effective_clusters < MIN_CLUSTERS_FOR_BOOTSTRAP {
        let mut approximation = wilson(numerator, denominator);
        approximation.effective_clusters = effective_clusters;
        return approximation;
    }

    let mut ratios = Vec::with_capacity(resamples);
    for _ in 0..resamples {
        let mut resampled_numerator = 0u64;
        let mut resampled_denominator = 0u64;
        for _ in 0..effective_clusters {
            let pick = usize::try_from(rng.below(effective_clusters as u64)).unwrap_or(0);
            resampled_numerator += contributing[pick].numerator;
            resampled_denominator += contributing[pick].denominator;
        }
        if resampled_denominator > 0 {
            ratios.push(resampled_numerator as f64 / resampled_denominator as f64);
        }
    }
    ratios.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    Proportion {
        numerator,
        denominator,
        ci_low: percentile(&ratios, 2.5).clamp(0.0, 1.0),
        ci_high: percentile(&ratios, 97.5).clamp(0.0, 1.0),
        estimator: ProportionEstimator::ClusterBootstrap,
        effective_clusters,
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

    // ── cluster bootstrap ────────────────────────────────────────────

    fn rng() -> crate::rng::Rng {
        crate::rng::Rng::derive(9, 9, crate::rng::RngDomain::Stats)
    }

    /// `count` 個桌次，每桌 `per_cluster` 個機會、`hits` 次命中
    fn uniform(count: usize, hits: u64, per_cluster: u64) -> Vec<ClusterCount> {
        vec![
            ClusterCount {
                numerator: hits,
                denominator: per_cluster,
            };
            count
        ]
    }

    #[test]
    fn 總比例相同但桌次聚集程度不同時區間不同() {
        // 兩份資料的總分子／分母完全一樣：50 個桌次、每桌 100 次機會、
        // 合計 2500/5000。差別只在命中怎麼分佈
        let even = uniform(50, 50, 100);
        // 極端聚集：一半桌次全中、一半全不中。使用者「在某些桌打得完全
        // 不一樣」正是這個形狀，證據強度遠低於平均分佈
        let clustered: Vec<ClusterCount> = (0..50)
            .map(|i| ClusterCount {
                numerator: if i % 2 == 0 { 100 } else { 0 },
                denominator: 100,
            })
            .collect();

        let a = cluster_bootstrap(&even, 500, &mut rng());
        let b = cluster_bootstrap(&clustered, 500, &mut rng());

        assert_eq!(a.numerator, b.numerator);
        assert_eq!(a.denominator, b.denominator);
        assert!(
            b.half_width().expect("有值") > a.half_width().expect("有值") * 5.0,
            "聚集資料 {:?} 的區間必須遠寬於平均分佈 {:?}——這正是不能把同桌手牌當獨立樣本的理由",
            b.half_width(),
            a.half_width()
        );
    }

    #[test]
    fn 群聚資料的區間寬於同一份資料的_wilson_近似() {
        let clustered: Vec<ClusterCount> = (0..40)
            .map(|i| ClusterCount {
                numerator: if i % 2 == 0 { 100 } else { 0 },
                denominator: 100,
            })
            .collect();
        let bootstrap = cluster_bootstrap(&clustered, 500, &mut rng());
        let independent = wilson(bootstrap.numerator, bootstrap.denominator);

        assert!(
            bootstrap.half_width().expect("有值") > independent.half_width().expect("有值"),
            "把 4000 手當成 4000 筆獨立觀測會低估不確定性"
        );
        assert_eq!(bootstrap.estimator, ProportionEstimator::ClusterBootstrap);
        assert_eq!(bootstrap.effective_clusters, 40);
    }

    #[test]
    fn 桌次不足時退回_wilson_並如實標示() {
        let few = uniform(MIN_CLUSTERS_FOR_BOOTSTRAP - 1, 30, 100);
        let p = cluster_bootstrap(&few, 500, &mut rng());

        assert_eq!(
            p.estimator,
            ProportionEstimator::WilsonIndependent,
            "桌次不足時不得冒充 cluster bootstrap"
        );
        assert_eq!(
            p.effective_clusters,
            MIN_CLUSTERS_FOR_BOOTSTRAP - 1,
            "退回近似時仍要說出手上有幾個桌次"
        );
        let reference = wilson(p.numerator, p.denominator);
        assert!((p.ci_low - reference.ci_low).abs() < 1e-12);
        assert!((p.ci_high - reference.ci_high).abs() < 1e-12);
    }

    #[test]
    fn 沒有分母的桌次不計入有效樣本() {
        let mut clusters = uniform(25, 30, 100);
        clusters.extend(vec![ClusterCount::default(); 10]);
        let p = cluster_bootstrap(&clusters, 200, &mut rng());
        assert_eq!(p.effective_clusters, 25, "沒有機會發生的桌次不是樣本");
        assert_eq!(p.denominator, 2_500);
    }

    #[test]
    fn 空輸入回傳_na_而不是崩潰() {
        let p = cluster_bootstrap(&[], 200, &mut rng());
        assert_eq!(p.point(), None);
        assert_eq!(p.effective_clusters, 0);
    }

    #[test]
    fn 相同_seed_的重抽結果一致() {
        let clusters: Vec<ClusterCount> = (0..30u64)
            .map(|i| ClusterCount {
                numerator: i % 7,
                denominator: 20,
            })
            .collect();
        let a = cluster_bootstrap(&clusters, 300, &mut rng());
        let b = cluster_bootstrap(&clusters, 300, &mut rng());
        assert_eq!(a, b, "統計重抽必須可重現");
    }

    #[test]
    fn 區間恆落在合法範圍且包含點估計() {
        for hits in [0u64, 1, 50, 99, 100] {
            let clusters = uniform(30, hits, 100);
            let p = cluster_bootstrap(&clusters, 300, &mut rng());
            let point = p.point().expect("分母非 0");
            assert!(p.ci_low >= 0.0 && p.ci_high <= 1.0, "{hits}/100 的區間越界");
            assert!(
                p.ci_low <= point && point <= p.ci_high,
                "{hits}/100 的區間未包含點估計"
            );
        }
    }
}
