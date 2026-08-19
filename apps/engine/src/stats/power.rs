//! 統計效力預覽（核心規格 5.3.1）。
//!
//! 規格要求：「UI 必須在使用者設定手數時**即時預覽該設定的預期可分辨差距**，
//! 不能等跑完才在報表揭露。」
//!
//! # 為什麼這件事重要
//!
//! `handLimit` 下限是 10K，但 10K 手不足以支撐大多數切片。使用者若在跑完
//! 100 萬手之後才知道「169 格熱力圖無法對任何一格做判定」，那 12 小時就白花了。
//! 預覽讓這個限制在**設定階段**就看得見。

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
    /// 95% CI 的半寬，bb/100
    pub half_width_bb100: f64,
    /// 該切片是否可能做出有意義的判定
    pub usable: bool,
}

/// 可用性門檻：半寬超過此值即視為無法支撐有意義的結論。
pub const USABLE_HALF_WIDTH_BB100: f64 = 10.0;

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
        usable: half_width_bb100 <= USABLE_HALF_WIDTH_BB100,
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
    fn 手數上限下_169_格仍無法判定() {
        // 核心規格 5.3.1 的關鍵結論：熱力圖在 1000K 上限下仍不可判定
        let preview = preview(1_000_000, AnalysisLevel::HandClassGrid, PLANNING_SIGMA_BB100);
        assert!(
            !preview.usable,
            "169 格在 100 萬手下仍不可判定，半寬 {:.1} bb/100",
            preview.half_width_bb100
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

    #[test]
    fn 手數下限下多數切片不可用() {
        // handLimit 下限為 10K
        let previews = preview_all(10_000, 9, PLANNING_SIGMA_BB100);
        let usable = previews.iter().filter(|p| p.usable).count();
        assert_eq!(
            usable, 0,
            "10K 手不足以支撐任何切片，這正是必須在設定階段預覽的理由"
        );
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
        assert!(!preview.usable);
    }

    #[test]
    fn 全層級預覽涵蓋規格表的四列() {
        let previews = preview_all(1_000_000, 9, PLANNING_SIGMA_BB100);
        assert_eq!(previews.len(), 4, "對應核心規格 5.3.1 的四個分析層級");
    }
}
