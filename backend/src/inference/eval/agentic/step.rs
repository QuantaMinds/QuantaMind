use crate::inference::eval::agentic::env_view::EnvView;
use serde::Serialize;

/// What happened on a single agent turn — drives the Trajectory Inspector's
/// per-step rendering and its red highlights for the error kinds.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    /// A valid call the sandbox recognized and answered.
    ToolCall,
    /// A recognized call the sandbox deliberately failed (Driver-B fault trap): an
    /// HTTP-style error is injected and the loop continues so a robust agent can
    /// retry (transient) or report the failure (persistent).
    ToolError,
    /// A parsed call the sandbox has no mock for (unknown tool or wrong args) — an
    /// error is injected and the loop continues.
    UnknownTool,
    /// Driver D: a schema-invalid call (missing required param / wrong type /
    /// unknown tool). A precise semantic correction is injected for recovery; the
    /// terminal SchemaError turn (no injection) means the recovery budget ran out.
    SchemaError,
    /// The model yielded with broken JSON where a call was attempted.
    MalformedJson,
    /// The model yielded (no call) without satisfying the EndStateRule — a fake
    /// "I'm done" (the lazy-agent failure).
    HallucinatedCompletion,
    /// The EndStateRule was satisfied — the success terminal.
    EndStateReached,
    /// The step cap was hit without success.
    InfiniteLoop,
    /// Phase 9-v2: the model invoked a `must_not_call` trap — terminal the instant
    /// it fires (the forbidden action never "happens").
    ForbiddenCall,
    /// Phase 9-v2: a model turn exceeded the per-step wall-clock budget (a stalled
    /// model) — terminal.
    TurnTimeout,
    /// G3: the model did all the required work but reported the answer in plain text
    /// instead of calling the required reporter tool — content correct, channel wrong.
    /// A failure, but the mildest (rendered amber, not the red of a true hallucination).
    ReportedInProse,
    /// The model emitted an unparseable foreign tool-call dialect (channel-token soup from
    /// a mis-built model) — neither valid JSON nor a recoverable native grammar. Named
    /// honestly so a template/dialect artifact isn't shown as a hallucination or bad JSON.
    ForeignDialect,
    /// The model produced no usable output — empty / whitespace / punctuation-only (e.g. a
    /// lone `.` before its stop token). A generation/template artifact, distinct from a
    /// hallucinated completion.
    EmptyOutput,
    /// The turn hit the per-turn `num_predict` cap (`finish_reason == "length"`) and, after a
    /// context-clamped retry, still produced no parseable call — the output was cut off, not a
    /// capability failure. Rendered as a harness-limit note ("Output Truncated at Token Cap"),
    /// distinct from MalformedJson / HallucinatedCompletion / EmptyOutput. When usage numbers are
    /// present, `Truncated` specifically means CONTEXT-BOUND (the window filled — a hardware limit).
    Truncated,
    /// The model spent its whole per-turn token budget reasoning and never emitted the call, while
    /// the context window still had room — a SETTING limit (raise the thinking preset) or genuine
    /// over-thinking, NOT memory. Rendered distinctly ("Reasoning-overrun") so the UI can say
    /// "raise a setting" instead of "buy a bigger machine".
    ReasoningOverrun,
}

/// One turn of an agentic run, streamed to the UI as it happens. `injection` is
/// the sandbox's text reply for this turn (`"Tool result: …"`), or `None` on a
/// terminal turn (end-state, yield, or loop cap).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct TrajectoryStep {
    pub run_index: u32,
    pub step_index: u32,
    pub raw_output: String,
    pub injection: Option<String>,
    pub kind: StepKind,
    /// A snapshot of the environment the agent acted on this turn, for the visual replay
    /// panel. `EnvView::None` for entity/static tasks (no replay). Streamed, never published.
    #[serde(default)]
    pub env: EnvView,
    /// Prompt tokens this turn served from the server's prompt cache (prefix reuse)
    /// vs recomputed — llama.cpp's `timings.cache_n`. `None` when the backend doesn't
    /// report it (Ollama/MLX) or the turn produced no model response (timeout/terminal).
    /// Surfaces per-turn prefix-reuse in the trace: a high value means the transcript
    /// prefix was reused (prefill ≈ 0) rather than re-prefilled.
    #[serde(default)]
    pub cache_n: Option<u32>,
    /// Prompt tokens actually PROCESSED (prefilled) this turn — llama.cpp's `timings.prompt_n`
    /// (`prompt_eval_count`), i.e. the RECOMPUTED count (the cached prefix is NOT counted here;
    /// total prompt = `cache_n + prefill_tokens`). With `cache_n` the trace shows reused-vs-
    /// recomputed and the green/amber prefix-reuse state. `None` when the backend doesn't report it.
    #[serde(default)]
    pub prefill_tokens: Option<u32>,
    /// Wall-clock spent prefilling this turn's prompt (`timings.prompt_ms`) — the cost a
    /// cache bust re-incurs. `None` when the backend doesn't report it / no model response.
    #[serde(default)]
    pub prefill_ms: Option<u64>,
    /// D9 usage accounting, populated on a `Truncated` / `ReasoningOverrun` turn so the UI can show
    /// BOTH bars and name which limit fired. `reasoning_tokens` = tokens this turn spent reasoning
    /// (≈ generated tokens when the answer was starved); `context_used`/`context_window` = how full
    /// the context window got (`context_used ≈ context_window` ⇒ hardware-bound). `None` otherwise.
    #[serde(default)]
    pub reasoning_tokens: Option<u32>,
    #[serde(default)]
    pub context_used: Option<u32>,
    #[serde(default)]
    pub context_window: Option<u32>,
}
