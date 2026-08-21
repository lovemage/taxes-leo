//! 統計效力預覽（核心規格 5.3.1）。
//!
//! 規格要求：「UI 必須在使用者設定手數時**即時預覽該設定的預期可分辨差距**，
//! 不能等跑完才在報表揭露。」
//!
//! # 為什麼這件事重要
//!
//! 使用者若在跑完之後才知道「169 格熱力圖的區間寬到什麼都看不出來」，
//! 那些時間就白花了。預覽讓精度在**設定階段**就看得見。
//!
//! # 一律計算，不擋
//!
//! 區間再寬也是資訊——「這個手數下只能分辨 ±30 bb/100」本身就是結論。
//! 因此本模組**永遠給出半寬**，不會因為寬就換成「樣本不足」四個字；
//! 那樣使用者什麼都不知道，還以為是程式壞了。
//!
//! 取而代之的是**建議手數**：告訴他跑到多少手才能把區間收到有說服力的
//! 程度。判斷留給使用者，但判斷所需的兩個數字都給齊。

/// 每手結果標準差的規劃用估計值，單位為 bb/100。
///
/// 核心規格 5.3.1：「以每手結果標準差 σ ≈ 90 bb/100 估算
/// （**M0 須以實測值取代**）」。
pub const PLANNING_SIGMA_BB100: f64 = 90.0;

/// 分析層級。切片數直接決定每個切片能分到多少手。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisLevel {
    /// 整體 bb/100
    Overall,
    /// 逐位置（單一在桌人數）
    PerPosition { seated: usize },
    /// 逐（在桌人數 × 位置）
    PerSeatedAndPosition,
    /// 169 格熱力圖
    HandClassGrid,
}

impl AnalysisLevel {
    /// 該層級的切片數。
    #[must_use]
    pub const fn slices(self) -> usize {
        match self {
            Self::Overall => 1,
            Self::PerPosition { seated } => seated,
            // 6+7+8+9 個位置槽
            Self::PerSeatedAndPosition => 30,
            Self::HandClassGrid => 169,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Overall => "整體 bb/100",
            Self::PerPosition { .. } => "逐位置",
            Self::PerSeatedAndPosition => "逐（在桌人數 × 位置）",
            Self::HandClassGrid => "169 格熱力圖",
        }
    }
}

/// 某設定下的效力預覽。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PowerPreview {
    pub level: AnalysisLevel,
    pub total_hands: u64,
    /// 每個切片分到的手數
    pub hands_per_slice: u64,
    /// 95% CI 的半寬，bb/100。**一律計算**，不因為寬就不給
    pub half_width_bb100: f64,
    /// 要把半寬收到 [`TARGET_HALF_WIDTH_BB100`] 所需的總手數
    pub hands_for_target: u64,
    /// 目前手數是否已達建議精度。**這不是「能不能用」的閘門**，
    /// 只是「結論有多少說服力」的標示
    pub meets_target: bool,
}

/// 建議精度：半寬收到此值以內，結論才具說服力。
///
/// **這是建議不是門檻。** 沒達到的切片照樣算出區間並顯示，
/// 只是同時告訴使用者「跑到 X 手會更有說服力」。
pub const TARGET_HALF_WIDTH_BB100: f64 = 10.0;

/// 計算某手數與分析層級下的預期可分辨差距。
///
/// 公式為獨立同分佈下的 `1.96σ/√(N/100)`。**這是樂觀下界**——
/// 核心規格 5.3.1：「moving block bootstrap 的 CI 通常比獨立同分佈估計
/// **更寬**，上表是樂觀下界，不是保證值。」
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn preview(total_hands: u64, level: AnalysisLevel, sigma_bb100: f64) -> PowerPreview {
    let slices = level.slices() as u64;
    let hands_per_slice = total_hands / slices.max(1);

    let half_width_bb100 = if hands_per_slice == 0 {
        f64::INFINITY
    } else {
        1.96 * sigma_bb100 / (hands_per_slice as f64 / 100.0).sqrt()
    };

    PowerPreview {
        level,
        total_hands,
        hands_per_slice,
        half_width_bb100,
        hands_for_target: hands_required(TARGET_HALF_WIDTH_BB100, level, sigma_bb100),
        meets_target: half_width_bb100 <= TARGET_HALF_WIDTH_BB100,
    }
}

/// 達到指定 CI 半寬所需的手數。
///
/// 供 UI 回答「要多少手才看得出 5 bb/100 的差距」。
#[must_use]
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn hands_required(half_width_bb100: f64, level: AnalysisLevel, sigma_bb100: f64) -> u64 {
    if half_width_bb100 <= 0.0 {
        return u64::MAX;
    }
    // 每個切片所需手數先取整，再乘切片數。手數無法是小數，
    // 先乘再取整會讓「逐位置需求 = 整體需求 × 位置數」這個關係差幾手
    let per_slice = ((1.96 * sigma_bb100 / half_width_bb100).powi(2) * 100.0).ceil() as u64;
    per_slice.saturating_mul(level.slices() as u64)
}

/// 產生全部層級的預覽，供面板 A 直接渲染。
#[must_use]
pub fn preview_all(total_hands: u64, seated: usize, sigma_bb100: f64) -> Vec<PowerPreview> {
    [
        AnalysisLevel::Overall,
        AnalysisLevel::PerPosition { seated },
        AnalysisLevel::PerSeatedAndPosition,
        AnalysisLevel::HandClassGrid,
    ]
    .into_iter()
    .map(|level| preview(total_hands, level, sigma_bb100))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 熱力圖在百萬手下仍達不到建議精度() {
        // 核心規格 5.3.1 的關鍵結論：熱力圖即使跑到 100 萬手，
        // 區間仍寬到不足以對單格做宣稱
        let preview = preview(1_000_000, AnalysisLevel::HandClassGrid, PLANNING_SIGMA_BB100);
        assert!(!preview.meets_target);
        assert!(
            preview.half_width_bb100.is_finite(),
            "達不到建議精度不代表算不出來——半寬照樣要給"
        );
    }

    #[test]
    fn 整體_bb100_在十萬手左右可達正負五() {
        // 規格表：整體 bb/100 半寬 ±5 需約 12.4 萬手
        let required = hands_required(5.0, AnalysisLevel::Overall, PLANNING_SIGMA_BB100);
        assert!(
            (120_000..130_000).contains(&required),
            "應約 12.4 萬手，實得 {required}"
        );
    }

    #[test]
    fn 逐位置需求約為整體的九倍() {
        let overall = hands_required(5.0, AnalysisLevel::Overall, PLANNING_SIGMA_BB100);
        let per_position =
            hands_required(5.0, AnalysisLevel::PerPosition { seated: 9 }, PLANNING_SIGMA_BB100);
        assert_eq!(per_position, overall * 9);
    }

    /// 手數很少時仍要給出區間與建議，不得以「樣本不足」帶過。
    #[test]
    fn 手數不足時仍算出半寬並給出建議手數() {
        // handLimit 下限為 1K
        let previews = preview_all(1_000, 9, PLANNING_SIGMA_BB100);
        assert_eq!(previews.iter().filter(|p| p.meets_target).count(), 0);

        for preview in &previews {
            assert!(
                preview.half_width_bb100.is_finite() && preview.half_width_bb100 > 0.0,
                "{} 的半寬必須算得出來，寬也要給",
                preview.level.as_str()
            );
            assert!(
                preview.hands_for_target > preview.total_hands,
                "{} 應建議一個比目前更大的手數",
                preview.level.as_str()
            );
        }
    }

    /// 達到建議精度後，建議手數不得超過目前手數。
    #[test]
    fn 達到建議精度時不再要求更多手數() {
        let preview = preview(100_000, AnalysisLevel::Overall, PLANNING_SIGMA_BB100);
        assert!(preview.meets_target, "10 萬手足以讓整體 bb/100 達到 ±10");
        assert!(preview.hands_for_target <= preview.total_hands);
    }

    #[test]
    fn 手數越多半寬越窄() {
        let small = preview(100_000, AnalysisLevel::Overall, PLANNING_SIGMA_BB100);
        let large = preview(1_000_000, AnalysisLevel::Overall, PLANNING_SIGMA_BB100);
        assert!(large.half_width_bb100 < small.half_width_bb100);
        // 手數增為 10 倍，半寬應縮為約 1/√10
        let ratio = small.half_width_bb100 / large.half_width_bb100;
        assert!((ratio - 10.0_f64.sqrt()).abs() < 0.01);
    }

    #[test]
    fn 零手數不崩潰() {
        let preview = preview(0, AnalysisLevel::Overall, PLANNING_SIGMA_BB100);
        assert!(preview.half_width_bb100.is_infinite());
        assert!(!preview.meets_target);
    }

    #[test]
    fn 全層級預覽涵蓋規格表的四列() {
        let previews = preview_all(1_000_000, 9, PLANNING_SIGMA_BB100);
        assert_eq!(previews.len(), 4, "對應核心規格 5.3.1 的四個分析層級");
    }
}
