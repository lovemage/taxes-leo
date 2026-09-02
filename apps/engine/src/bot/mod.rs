//! Bot 決策層。
//!
//! 核心規格 4.3 的兩層參數與固定決策管線。

pub mod agent;
pub mod params;
pub mod pipeline;

pub use params::{ParamError, ParamSpec, ParamValue, BEHAVIOR_SPECS, PERSONA_SPECS};
pub use agent::{
    rules_for_bot, scenario_of, BotAgent, MAX_EXPECTED_OPPONENTS, POSTFLOP_BASELINE_VERSION,
    POSTFLOP_EQUITY_SAMPLES, POSTFLOP_FALLBACK_VERSION,
};
pub use pipeline::{BotConfig, DecisionTrace, PipelineStage};
