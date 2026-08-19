//! 多重比較控制（Benjamini–Hochberg FDR）。
//!
//! 核心規格 5.3：「169 格與大量情境切片預設標示探索性；若產生『漏洞』旗標，
//! 使用 **BH-FDR** 或預先登錄的多重比較控制。」
//!
//! # 為什麼不能省
//!
//! 169 格熱力圖同時做 169 次檢定，在 α=0.05 下即使策略完全無漏洞，
//! 期望仍會有約 8 格「顯著」。不做多重比較控制就宣稱找到漏洞，
//! 等於保證每次都會找到不存在的漏洞。

/// 一項檢定的結果。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Test {
    /// 切片識別，供呼叫端對回原始資料
    pub id: usize,
    pub p_value: f64,
}

/// BH 程序的判定結果。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FdrResult {
    pub id: usize,
    pub p_value: f64,
    /// 是否在控制 FDR 後仍達顯著
    pub significant: bool,
}

/// Benjamini–Hochberg 程序。
///
/// 回傳與輸入等長的結果，順序與輸入一致。`alpha` 為期望的偽發現率。
///
/// # Panics
/// `alpha` 不在 `(0, 1)` 之間時 panic。
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn benjamini_hochberg(tests: &[Test], alpha: f64) -> Vec<FdrResult> {
    assert!(alpha > 0.0 && alpha < 1.0, "alpha 必須在 (0,1) 之間");

    if tests.is_empty() {
        return Vec::new();
    }

    let m = tests.len();
    let mut ordered: Vec<(usize, f64)> = tests
        .iter()
        .enumerate()
        .map(|(i, t)| (i, t.p_value))
        .collect();
    ordered.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    // 找出最大的 k，使 p(k) <= k/m * alpha；該名次以前全部拒絕虛無假設
    let mut cutoff_rank = 0usize;
    for (rank, &(_, p)) in ordered.iter().enumerate() {
        let threshold = (rank + 1) as f64 / m as f64 * alpha;
        if p <= threshold {
            cutoff_rank = rank + 1;
        }
    }

    let mut significant = vec![false; m];
    for &(index, _) in ordered.iter().take(cutoff_rank) {
        significant[index] = true;
    }

    tests
        .iter()
        .enumerate()
        .map(|(i, t)| FdrResult {
            id: t.id,
            p_value: t.p_value,
            significant: significant[i],
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss)]
mod tests {
    use super::*;

    fn tests_from(p_values: &[f64]) -> Vec<Test> {
        p_values
            .iter()
            .enumerate()
            .map(|(id, &p_value)| Test { id, p_value })
            .collect()
    }

    #[test]
    fn 全部為虛無時幾乎不產生旗標() {
        // 均勻分佈的 p 值代表沒有真實效應
        let uniform: Vec<f64> = (1..=169).map(|i| f64::from(i) / 170.0).collect();
        let results = benjamini_hochberg(&tests_from(&uniform), 0.05);
        let flagged = results.iter().filter(|r| r.significant).count();
        assert_eq!(
            flagged, 0,
            "169 個純虛無檢定不應產生任何漏洞旗標，實得 {flagged}"
        );
    }

    #[test]
    fn 未控制多重比較會產生假陽性() {
        // 同一組資料，若用未校正的 alpha=0.05 逐一判定
        let uniform: Vec<f64> = (1..=169).map(|i| f64::from(i) / 170.0).collect();
        let naive = uniform.iter().filter(|&&p| p <= 0.05).count();
        assert!(
            naive > 0,
            "未校正時會有 {naive} 個假陽性，這正是必須做 FDR 控制的理由"
        );
    }

    #[test]
    fn 明確的強效應會被保留() {
        let mut p_values = vec![0.9; 100];
        p_values[0] = 0.000_01;
        p_values[1] = 0.000_02;
        let results = benjamini_hochberg(&tests_from(&p_values), 0.05);
        assert!(results[0].significant, "極小的 p 值應通過 FDR");
        assert!(results[1].significant);
        assert!(!results[50].significant);
    }

    #[test]
    fn 回傳順序與輸入一致() {
        let p_values = [0.5, 0.001, 0.9, 0.02];
        let results = benjamini_hochberg(&tests_from(&p_values), 0.05);
        for (i, result) in results.iter().enumerate() {
            assert_eq!(result.id, i, "順序必須與輸入一致，供呼叫端對回原始資料");
            assert!((result.p_value - p_values[i]).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn 空輸入不崩潰() {
        assert!(benjamini_hochberg(&[], 0.05).is_empty());
    }

    #[test]
    fn bh_比_bonferroni_寬鬆但仍受控() {
        // 20 個檢定，其中 5 個有中等強度效應
        let mut p_values = vec![0.6; 20];
        for slot in p_values.iter_mut().take(5) {
            *slot = 0.008;
        }
        let results = benjamini_hochberg(&tests_from(&p_values), 0.05);
        let flagged = results.iter().filter(|r| r.significant).count();

        assert_eq!(flagged, 5, "BH 應保留這 5 個中等效應");

        // 同一組 p 值用 Bonferroni（門檻 0.05/20 = 0.0025）會全部漏掉。
        // 實際算出來而非寫成恆真的常數比較，才驗得到東西
        let bonferroni_threshold = 0.05 / p_values.len() as f64;
        let bonferroni_flagged = p_values
            .iter()
            .filter(|&&p| p <= bonferroni_threshold)
            .count();
        assert_eq!(
            bonferroni_flagged, 0,
            "Bonferroni 會漏掉全部 5 個真實效應，這是 BH 較適合大量切片的理由"
        );
    }
}
