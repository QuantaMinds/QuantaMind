use super::vram_fit::MemoryProfile;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::eval::agentic::scoring::report::FailureTracker;
use crate::inference::eval::agentic::spec::Tier;
use crate::inference::eval::batch::TierStat;
use serde::{Deserialize, Serialize};

/// Absorbs float drift (a serialized `0.80` read back as `0.7999999999999999`,
/// or a true 4/5 = 0.8) so a threshold comparison never flips a false NotReady.
pub const EPSILON: f64 = 1e-6;

/// The context-cliff outcome for a (collection, model) — the single source of truth
/// the readiness gate and the report read (replacing a bare `Option<u32>` that
/// couldn't tell "probed, held" from "never probed"). `NotProbed` = no probe run
/// (absence in the store); `NoCliff { tested }` = accuracy held through `tested`
/// tokens; `Collapsed { depth }` = tool-call accuracy fell off at `depth` tokens (a
/// "broken baseline" maps here at the first rung — it fails from the start).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum CliffStatus {
    #[default]
    NotProbed,
    NoCliff { tested: u32 },
    Collapsed { depth: u32 },
    /// Tool-call accuracy was already failing at the SMALLEST tested context (a broken
    /// baseline) — there is no usable context window, distinct from a collapse partway
    /// through. `tested` = the deepest rung reached. Renders red "fails from start" and
    /// blocks any context-headroom gate.
    Broken { tested: u32 },
}

/// Which measurement path produced the verdict — stated explicitly so a "Ready"
/// on the prompt-based proxy is never mistaken for the native tool-calling path.
/// `PromptBased` is the default: the conservative path that works on any backend.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentPath {
    #[default]
    PromptBased,
    NativeFc,
}

/// Native function-calling result for the targeted backend. `NotSupported` ⇒ the
/// report shows N/A; we never synthesize a Pass^k for an unmeasured path.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeFcStatus {
    Tested { pass_k: f64 },
    NotSupported,
}

/// The measured facts a verdict is computed from. `Option` ⇒ "not measured" — a
/// hard-required-but-`None` metric blocks; a soft-target-but-`None` is a note.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadinessInputs {
    pub pass_k: Option<f64>,
    pub avg_steps: Option<f64>,
    pub ms_per_step: Option<u64>,
    pub cliff: CliffStatus,
    pub fits_in_vram: Option<bool>,
    /// Fits, but sits near the allocation ceiling → a soft Conditional note.
    pub vram_pressure: bool,
    pub loops: u32,
    pub hallucinated: u32,
    /// The MODEL-LEVEL native function-calling capability — `Tested` when the column's
    /// native pass actually ran, else `NotSupported`. Identical for BOTH of a column's
    /// per-path rows (native availability is a property of the model, not of the row's
    /// path), so the `require_native_fc` gate reads the same value whether this row is
    /// the native or the prompt-based path. Decoupled from `path` below.
    pub native_fc: NativeFcStatus,
    /// Which path THIS row represents — set explicitly, NOT derived from `native_fc`.
    /// A native-capable model's prompt-based row carries `native_fc = Tested` (so the
    /// gate doesn't falsely block it) but `path = PromptBased` (so the label is honest).
    /// `#[serde(default)]` → pre-existing inputs deserialize as `PromptBased`.
    #[serde(default)]
    pub path: AgentPath,
    /// Phase 9: strict Pass^k per difficulty tier that was actually exercised
    /// (sorted ascending by tier). `assess` derives the cleared tier from this and
    /// compares it to the profile's `required_tier`. Empty for a collection with no
    /// tiered tasks → the tier gate stays silent (a tier can't be failed if it was
    /// never run). `#[serde(default)]` so pre-Phase-9 inputs deserialize.
    #[serde(default)]
    pub tier_pass_k: Vec<(Tier, f64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Readiness {
    Ready,
    Conditional,
    NotReady,
}

/// A transparent verdict: the status plus the exact reasons (with interpolated
/// thresholds) that produced it, and the path it was measured on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadinessVerdict {
    pub status: Readiness,
    pub blocking: Vec<String>,
    pub conditions: Vec<String>,
    pub path: AgentPath,
    /// Phase 9: the difficulty tier this profile requires (graduated readiness —
    /// the report shows "cleared X / requires Y", never a bare pass/fail).
    #[serde(default)]
    pub required_tier: Tier,
    /// Phase 9: the highest tier the model actually cleared at the profile's bar
    /// (`pass^k ≥ min_pass_k`). `None` when no tier cleared, or no tiered task ran.
    #[serde(default)]
    pub cleared_tier: Option<Tier>,
}

/// A verdict paired with the model it judged — one row of the Agent Report. The
/// `memory` profile is present when VRAM fit was measured (Ollama with a cap),
/// `None` for single-model backends or when no cap was supplied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelVerdict {
    pub model: String,
    pub backend: BackendKind,
    pub verdict: ReadinessVerdict,
    #[serde(default)]
    pub memory: Option<MemoryProfile>,
    /// Efficiency telemetry for the recommender ranking (Phase 7.3), sourced
    /// native-first — the same aggregate the verdict was computed from. `None` when
    /// the model had no agentic run (sinks to the bottom of the ranking).
    #[serde(default)]
    pub avg_steps: Option<f64>,
    #[serde(default)]
    pub effort: Option<f64>,
    /// The measured Pass^k the verdict gated on (native-first), as a raw fraction —
    /// the headline reliability metric for the row. `None` when no agentic run was
    /// measured (rendered "N/A", never fabricated).
    #[serde(default)]
    pub pass_k: Option<f64>,
    /// The model's real quantization (e.g. `Q4_K_M`) from the installed-models
    /// registry — never guessed. `None` when the backend didn't report one.
    #[serde(default)]
    pub quantization: Option<String>,
    /// The model's context-cliff outcome for this collection, from the cliff store:
    /// NotProbed (rendered "N/A"), NoCliff{tested} ("✓ No cliff (≥tested)"), or
    /// Collapsed{depth} ("Collapsed at depth"). The gate only blocks when a profile
    /// opts in via `min_context_tokens` (strict: NoCliff passes iff tested ≥ min).
    #[serde(default)]
    pub cliff: CliffStatus,
    /// Phase 9B: per-tier strict Pass^k + avg-steps + failures (native-first — the same
    /// aggregate the verdict gated on), powering the Agent Report's Tier Progression
    /// Matrix. Empty when no agentic run was measured. `#[serde(default)]` for back-compat.
    #[serde(default)]
    pub by_tier: Vec<TierStat>,
    /// Phase 9B: the model's overall failure breakdown (native-first), for the Failure
    /// Taxonomy. `#[serde(default)]` so pre-9B verdicts deserialize.
    #[serde(default)]
    pub failures: FailureTracker,
    /// The RUN-LEVEL pass tally across the collection: `passes` of `total_runs` individual
    /// runs succeeded. Distinct from `pass_k`, which is the STRICT all-k gate (fraction of
    /// tasks where EVERY run passed) — so a 14/16 shows `pass_k = 0` yet `passes/total_runs`
    /// = 14/16. Surfaced beside the strict badge so a merely-very-good model doesn't read as a
    /// flat failure. `#[serde(default)]` (0/0 → rendered "N/A") for back-compat.
    #[serde(default)]
    pub passes: u32,
    #[serde(default)]
    pub total_runs: u32,
}
