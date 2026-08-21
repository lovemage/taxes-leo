//! Bot 決策層。
//!
//! 核心規格 4.3 的兩層參數與固定決策管線。

pub mod agent;
pub mod params;
pub mod pipeline;

pub use params::{ParamError, ParamSpec, ParamValue, BEHAVIOR_SPECS, PERSONA_SPECS};
pub use agent::{scenario_of, BotAgent, POSTFLOP_FALLBACK_VERSION};
pub use pipeline::{BotConfig, DecisionTrace, PipelineStage};
