use crate::inference::eval::agentic::difficulty::passk::{pass_k_for, think_tokens_for_preset, ThinkPreset};
use crate::inference::eval::agentic::scoring::report::FailureTracker;
use crate::inference::eval::agentic::spec::Tier;
use crate::inference::eval::readiness::hardware::hwclass::HardwareClass;
use crate::inference::eval::readiness::types::{AgentPath, ModelVerdict, Readiness};
use crate::persistence::prompts::schema::InferenceParams;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The publish-payload schema version, stamped on every row so the server can parse
/// old submissions as the shape evolves. `1` = the Phase 9 extension (tier verdict,
/// per-tier curve, failure distribution). `2` = the reasoning-budget extension
/// (is_thinking, think_preset, think_budget, ctx_ceiling, cpu_offloaded).
pub const PUBLISH_SCHEMA_VERSION: u32 = 2;

/// The metrics actually published — the few aggregates the leaderboard ranks and
/// the baseline worker percentiles. `pass_k` is the required headline reliability
/// metric; `effort`/`avg_steps` are soft and omitted when unmeasured (the JSONB
/// bag stays additive/forward-compatible). No task content ever lives here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublishMetrics {
    pub pass_k: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_steps: Option<f64>,
}

/// One tier's point on the saturation curve — the per-tier evidence behind the
/// headline verdict. `pass_k_rate` is the strict tasks-passed/tasks-total fraction,
/// `k` the repetitions that tier was scored at (5/8/16/24), `decoy_count` the number
/// of distractor tools the tier presented (from the collection's task axes; `None`
/// when the collection declares none). No task content — just the shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TierMetric {
    pub tier: Tier,
    pub pass_k_rate: f64,
    pub k: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_steps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decoy_count: Option<u32>,
}

/// The failure distribution — counts per mode, a distribution and never the failing
/// runs themselves. Built by an EXPLICIT field-by-field map from `FailureTracker`
/// (see `from_tracker`), NOT by serializing the tracker: a new internal failure
/// counter must be deliberately added here to ever leave the machine (allowlist).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureDistribution {
    pub infinite_loop: u32,
    pub hallucinated: u32,
    pub malformed_json: u32,
    pub schema_unrecovered: u32,
    pub unknown_tool_calls: u32,
    pub forbidden_calls: u32,
    pub turn_timeouts: u32,
    pub reported_in_prose: u32,
}

impl FailureDistribution {
    /// Map the internal tracker into the published distribution, one named field at a
    /// time. Adding a field to `FailureTracker` does NOT auto-publish it — it must be
    /// wired here on purpose.
    fn from_tracker(f: &FailureTracker) -> Self {
        FailureDistribution {
            infinite_loop: f.infinite_loop_hits,
            hallucinated: f.hallucinated_completions,
            malformed_json: f.malformed_json_calls,
            schema_unrecovered: f.schema_unrecovered_calls,
            unknown_tool_calls: f.unknown_tool_calls,
            forbidden_calls: f.forbidden_calls,
            turn_timeouts: f.turn_timeouts,
            reported_in_prose: f.reported_in_prose_calls,
        }
    }
}

/// The run-wide context a single batch shares, assembled once by the command layer
/// and threaded into every `project` call. Carries the identifiers that are the same
/// for the whole batch (cohort, collection identity, provenance) plus the
/// hardware-derived advisory and the per-tier decoy axes. `collection_hash` is
/// `None` for a non-built-in collection — the signal that a custom/user-authored
/// collection's rows must be excluded.
pub struct PublishContext {
    pub params: InferenceParams,
    pub cohort_key: String,
    pub engine_version: String,
    pub build_hash: String,
    pub collection_name: String,
    pub collection_hash: Option<String>,
    pub decoys_by_tier: BTreeMap<Tier, u32>,
    pub hardware_class: HardwareClass,
    pub recommended_tier: Tier,
}

/// One publishable row: a model's identity (model + quant + hardware cohort), the
/// graduated tier verdict, the per-tier saturation curve, the failure distribution,
/// the collection identity + build provenance the numbers were measured under, and
/// the inference `params` of the run. This is the WHOLE wire shape, built by
/// ALLOWLIST in `project` — anything in `ModelVerdict` not named here (verdict
/// reasons, memory profile, backend internals, traces) is deliberately dropped, so a
/// new `ModelVerdict` field is private until someone adds it here on purpose.
///
/// Run-wide fields (collection identity, schema/engine/build versions) are repeated
/// per row, matching the existing `cohort_key`/`tool_version` precedent, so the
/// canonical hash stays a single hash over `[PublishRow]` and the server keeps
/// re-hashing `results` unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublishRow {
    pub model: String,
    pub quant: String,
    pub cohort_key: String,
    pub tool_version: String,
    pub metrics: PublishMetrics,
    pub params: InferenceParams,
    // Verdict — the headline the leaderboard ranks on.
    pub status: Readiness,
    pub eval_method: AgentPath,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier_tested: Option<Tier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleared_tier: Option<Tier>,
    pub hardware_class: HardwareClass,
    pub recommended_tier: Tier,
    pub by_tier: Vec<TierMetric>,
    pub failure_distribution: FailureDistribution,
    // Reasoning-budget context — so the board can interpret the token `effort` and the
    // per-tier reliability of a reasoning model (a thinking model legitimately spends far
    // more tokens; a small box gets a smaller window). All measured, never fabricated.
    pub is_thinking: bool,
    pub think_preset: ThinkPreset,
    /// The reasoning-scratchpad token cap at the tier tested — real when the model reasons,
    /// `None` for a terse model (never a fake 0). Omitted from the wire when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub think_budget: Option<u32>,
    /// The hardware-adaptive `num_ctx` ceiling the run used. Omitted when unrecorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ctx_ceiling: Option<u32>,
    pub cpu_offloaded: bool,
    // Collection identity + provenance — so results are only compared across an
    // identical scenario set, and the board can dedup/verify by build.
    pub collection_name: String,
    pub collection_hash: String,
    pub schema_version: u32,
    pub engine_version: String,
    pub build_hash: String,
}

impl PublishRow {
    /// Project a verdict + the batch context into the publish row. Returns `None`
    /// (excluded from the batch, never sent as a null that would skew the server's
    /// baseline `n`/percentiles) when the model has no measured `pass_k`, no real
    /// quantization, OR the collection isn't a built-in (`ctx.collection_hash` is
    /// `None` — custom/user-authored collections never publish).
    pub fn project(v: &ModelVerdict, ctx: &PublishContext) -> Option<PublishRow> {
        let pass_k = v.pass_k?;
        let quant = v.quantization.clone()?;
        let collection_hash = ctx.collection_hash.clone()?;

        let by_tier = v
            .by_tier
            .iter()
            .map(|ts| TierMetric {
                tier: ts.tier,
                pass_k_rate: ts.pass_k().unwrap_or(0.0),
                k: pass_k_for(ts.tier),
                avg_steps: ts.avg_steps,
                decoy_count: ctx.decoys_by_tier.get(&ts.tier).copied(),
            })
            .collect();
        let tier_tested = v.by_tier.iter().map(|ts| ts.tier).max();
        // The reasoning-scratchpad token cap at the tier tested — the honest "how much room did it
        // get to think" number. Only for a reasoning model at a tested tier; `None` (never a fake 0)
        // for a terse model or an untiered run.
        let think_budget = v
            .is_thinking
            .then(|| tier_tested.map(|t| think_tokens_for_preset(t, v.think_preset)))
            .flatten();

        Some(PublishRow {
            model: v.model.clone(),
            quant,
            cohort_key: ctx.cohort_key.clone(),
            tool_version: ctx.engine_version.clone(),
            metrics: PublishMetrics { pass_k, effort: v.effort, avg_steps: v.avg_steps },
            params: ctx.params.clone(),
            status: v.verdict.status,
            eval_method: v.verdict.path,
            tier_tested,
            cleared_tier: v.verdict.cleared_tier,
            hardware_class: ctx.hardware_class,
            recommended_tier: ctx.recommended_tier,
            by_tier,
            failure_distribution: FailureDistribution::from_tracker(&v.failures),
            is_thinking: v.is_thinking,
            think_preset: v.think_preset,
            think_budget,
            ctx_ceiling: v.ctx_ceiling,
            cpu_offloaded: v.cpu_offloaded,
            collection_name: ctx.collection_name.clone(),
            collection_hash,
            schema_version: PUBLISH_SCHEMA_VERSION,
            engine_version: ctx.engine_version.clone(),
            build_hash: ctx.build_hash.clone(),
        })
    }
}

#[cfg(test)]
impl PublishContext {
    /// A built-in-collection context for tests: a real cohort/version, a present
    /// `collection_hash` (so measured rows project), and empty decoy axes. Fields are
    /// public, so a test mutates what it cares about (params, decoys, `collection_hash`
    /// = `None` to exercise the custom-collection exclusion).
    pub fn test_ctx(cohort: &str, version: &str) -> PublishContext {
        PublishContext {
            params: InferenceParams::default(),
            cohort_key: cohort.to_string(),
            engine_version: version.to_string(),
            build_hash: "testhash".to_string(),
            collection_name: "easy-coding".to_string(),
            collection_hash: Some("c0ffee".to_string()),
            decoys_by_tier: BTreeMap::new(),
            hardware_class: HardwareClass::Mainstream,
            recommended_tier: Tier::Medium,
        }
    }
}

#[cfg(test)]
impl PublishRow {
    /// A minimal valid sample row for tests that exercise validation/canonicalization
    /// directly (not via `project`): a measured, quantized, built-in-collection row.
    /// Fields are public, so a test mutates the one it's probing.
    pub fn sample(model: &str, pass_k: f64) -> PublishRow {
        PublishRow {
            model: model.to_string(),
            quant: "Q4_K_M".to_string(),
            cohort_key: "apple-silicon/m-series/32-64gb".to_string(),
            tool_version: "0.2.0".to_string(),
            metrics: PublishMetrics { pass_k, effort: Some(1.2), avg_steps: Some(3.0) },
            params: InferenceParams::default(),
            status: Readiness::Ready,
            eval_method: AgentPath::NativeFc,
            tier_tested: Some(Tier::Medium),
            cleared_tier: Some(Tier::Medium),
            hardware_class: HardwareClass::Mainstream,
            recommended_tier: Tier::Medium,
            by_tier: Vec::new(),
            failure_distribution: FailureDistribution {
                infinite_loop: 0,
                hallucinated: 0,
                malformed_json: 0,
                schema_unrecovered: 0,
                unknown_tool_calls: 0,
                forbidden_calls: 0,
                turn_timeouts: 0,
                reported_in_prose: 0,
            },
            is_thinking: false,
            think_preset: ThinkPreset::default(),
            think_budget: None,
            ctx_ceiling: None,
            cpu_offloaded: false,
            collection_name: "easy-coding".to_string(),
            collection_hash: "abc".to_string(),
            schema_version: PUBLISH_SCHEMA_VERSION,
            engine_version: "0.2.0".to_string(),
            build_hash: "testhash".to_string(),
        }
    }
}

#[cfg(test)]
#[path = "row_tests.rs"]
mod tests;
