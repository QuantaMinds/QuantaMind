use crate::inference::backend::backend_kind::BackendKind;
use serde::{Deserialize, Serialize};

/// One recorded model run for a collection: when it ran, against what, and the
/// four sub-scores + composite. The unit the regression timeline plots over time.
///
/// This is a **domain** type — produced by the eval engine
/// (`batch::batch_summaries`, `toolcall::matrix::summaries`) and persisted by the
/// driven adapter `persistence::eval_history`. It lives in the domain so the
/// dependency edge points one way (`persistence → inference`), never back.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RunSummary {
    pub ts: String,
    pub model: String,
    pub backend: BackendKind,
    pub parse_rate: Option<f64>,
    pub tool_selection_acc: Option<f64>,
    pub arg_acc: Option<f64>,
    pub abstain_acc: Option<f64>,
    pub composite: Option<f64>,
    pub n: usize,
    /// Agentic Pass^k rate (passes / total_runs) for the run, when the collection
    /// had agentic tasks. `#[serde(default)]` so single-turn matrix history written
    /// before Phase 6 still loads.
    #[serde(default)]
    pub pass_k: Option<f64>,
    #[serde(default)]
    pub agentic_avg_steps: Option<f64>,
    #[serde(default)]
    pub effort: Option<f64>,
    /// The run used a reasoning model (thinking toggle). `effort` above is then a
    /// thinking model's token count and must not be compared against a terse model's on
    /// the regression timeline. `#[serde(default)]` so pre-existing history loads as `false`.
    #[serde(default)]
    pub is_thinking: bool,
}
