//! 統計層。
//!
//! 核心規格第五章。這一層的職責不只是算數字，更是**讓數字帶著它的可信度**：
//! 每個估計都附 estimator 名稱、有效樣本與可判定狀態，
//! 呼叫端無法只拿走點估計而丟掉脈絡。
//!
//! 統計主體只有使用者座位（核心規格 5.0）。

pub mod bootstrap;
pub mod fdr;
pub mod power;
pub mod proportion;

pub use bootstrap::{
    estimate_block_length, moving_block_bootstrap, Estimate, Estimator, Observation, Verdict,
    DEFAULT_RESAMPLES, MIN_EFFECTIVE_BLOCKS,
};
pub use fdr::{benjamini_hochberg, FdrResult, Test};
pub use power::{
    hands_required, preview, preview_all, AnalysisLevel, PowerPreview, PLANNING_SIGMA_BB100,
};
pub use proportion::{wilson, Proportion};
