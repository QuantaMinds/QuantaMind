//! Context-cliff probe (Tauri-free). Pads a tool-call task to growing verified
//! token depths, injects the instruction at swept positions, and reports the
//! largest context where accuracy still holds. The command layer wraps this with
//! events + persistence; the engine here has no UI dependency.

pub mod budget;
pub mod engine;
pub mod padding;
pub mod presets;
pub mod stats;

pub use budget::{tier_for_depth, CliffBudget, CLIFF_ANSWER_TOKENS, CLIFF_BASE_HEADROOM};
pub use engine::{build_ladder, run_cliff, run_cliff_with, run_cliff_with_factory, NO_CTX_LIMIT, CliffPoint, CliffReport, DepthScore, StepProgress, TaskTally, TaskTrace, TraceOutput, DEFAULT_DEPTHS};
pub use presets::{CliffPreset, CliffSource};
