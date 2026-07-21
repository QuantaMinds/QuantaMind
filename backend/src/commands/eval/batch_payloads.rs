use crate::inference::eval::agentic::step::TrajectoryStep;
use crate::inference::eval::batch::{BatchReport, TaskOutcome};
use serde::Serialize;

pub const EVENT_BATCH_PROGRESS: &str = "batch-progress";
pub const EVENT_AGENTIC_STEP: &str = "agentic-step";
pub const EVENT_BATCH_COMPLETE: &str = "batch-complete";

/// Per-task progress on the single `batch-progress` stream the frontend listens
/// to once. `Started` carries `total` (sizes the progress bar); `Done` carries
/// the task's outcome (cached for the trace debugger).
#[derive(Serialize, Clone)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum BatchProgress {
    Started { collection_id: String, model: String, task_id: String, index: usize, total: usize, category: String, is_native: bool },
    /// `is_native` tags the NATIVE pass's per-task result so the UI fills its native column
    /// separately, streamed as each native task finishes (the prompt pass uses `false`).
    Done { collection_id: String, model: String, task_id: String, outcome: TaskOutcome, is_native: bool },
}

/// A live agentic turn, tagged so the trace debugger routes it to the right
/// (model, task) trajectory. `collection_id` completes the attribution triple the
/// Inspector's Test-run latency view keys on — an event is now traceable to
/// (collection, task, model) without any out-of-band context.
#[derive(Serialize, Clone)]
pub struct AgenticStepPayload {
    pub collection_id: String,
    pub model: String,
    pub task_id: String,
    /// Which pass produced this turn — the native function-calling pass (`true`) or the prompt
    /// pass (`false`). The UI renders the two trajectories as separate sections.
    pub is_native: bool,
    #[serde(flatten)]
    pub step: TrajectoryStep,
}

#[derive(Serialize, Clone)]
pub struct BatchCompletePayload {
    pub report: BatchReport,
    /// `false` for an INTERMEDIATE complete (the native pass's result before the prompt pass,
    /// or a resume's partial replay) — the run is still going, so the UI keeps "running" true
    /// and shows pending cells as "Running…". `true` only on the last complete of the run.
    pub r#final: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::eval::agentic::env_view::{EnvView, FsNode, FsOp, FsView};
    use crate::inference::eval::agentic::step::StepKind;

    #[test]
    fn agentic_step_payload_carries_env_at_the_top_level_for_the_frontend() {
        // The frontend destructures `{model, task_id, ...step}` and reads `step.env`. With
        // `#[serde(flatten)]`, the step fields (INCLUDING env) must appear at the payload's top
        // level, and env must serialize as the internally-tagged object the Zod schema expects.
        let payload = AgenticStepPayload {
            collection_id: "easy-coding".into(),
            model: "m".into(),
            task_id: "t".into(),
            is_native: false,
            step: TrajectoryStep {
                run_index: 0,
                step_index: 1,
                raw_output: String::new(),
                injection: None,
                kind: StepKind::ToolCall,
                env: EnvView::FileSystem(FsView {
                    tree: vec![FsNode { path: "config.yaml".into(), is_dir: false }],
                    focus_path: Some("config.yaml".into()),
                    op: FsOp::Read,
                    content: Some("timeout: 30".into()),
                    matches: vec![],
                }),
                cache_n: Some(42),
                prefill_tokens: Some(8),
                prefill_ms: Some(12),
                eval_ms: Some(950),
                load_ms: Some(0),
                total_ms: Some(1103),
                output_tokens: Some(40),
                resident_bytes: Some(7_000_000_000),
                reasoning_tokens: Some(299),
                thinking_split_measured: true,
                context_used: None,
                context_window: None,
                initial_prompt: None,
            },
        };
        let v = serde_json::to_value(&payload).unwrap();
        // Flattened: step fields at top level.
        assert_eq!(v["kind"], "tool_call");
        assert_eq!(v["run_index"], 0);
        // The attribution triple is complete on the wire: (collection, task, model).
        assert_eq!(v["collection_id"], "easy-coding");
        assert_eq!(v["task_id"], "t");
        assert_eq!(v["model"], "m");
        // Per-turn prefix-cache fields flatten to the top level too (the trace reads them to
        // show reused-vs-recomputed; total prompt = cache_n + prefill_tokens = 42 + 8 = 50).
        assert_eq!(v["cache_n"], 42);
        assert_eq!(v["prefill_tokens"], 8);
        assert_eq!(v["prefill_ms"], 12);
        // Turn-cost fields (prefill/decode split + generated count) flatten too — the
        // Latency view's per-task step track reads them off the same event.
        assert_eq!(v["eval_ms"], 950);
        assert_eq!(v["load_ms"], 0);
        assert_eq!(v["total_ms"], 1103);
        assert_eq!(v["output_tokens"], 40);
        assert_eq!(v["resident_bytes"], 7_000_000_000u64);
        // The measured thinking split flattens with its provenance flag.
        assert_eq!(v["reasoning_tokens"], 299);
        assert_eq!(v["thinking_split_measured"], true);
        // env present and tagged.
        assert_eq!(v["env"]["kind"], "file_system");
        assert_eq!(v["env"]["op"], "read");
        assert_eq!(v["env"]["content"], "timeout: 30");
        assert_eq!(v["env"]["focus_path"], "config.yaml");
    }
}
