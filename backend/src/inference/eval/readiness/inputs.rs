use super::profile::ReadinessProfile;
use super::types::{AgentPath, CliffStatus, ModelVerdict, NativeFcStatus, Readiness, ReadinessInputs, ReadinessVerdict};
use super::verdict::assess;
use super::vram_fit::MemoryProfile;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::eval::agentic::difficulty::passk::ThinkPreset;
use crate::inference::eval::batch::{AggAgentic, BatchColumn, BatchReport, TierStat};
use crate::inference::gguf::gguf_quant::quant_from_filename;

/// The native-first agentic aggregate — the NATIVE pass when it actually ran (`total_runs
/// > 0`), else the prompt-based proxy. The single selector every readiness read shares, so
/// the gated metrics, the per-tier breakdown, and the failure taxonomy can never come from
/// different passes (native-FC defaults off, so both must fall back identically).
pub fn native_first_source(col: &BatchColumn) -> Option<&AggAgentic> {
    col.agentic_native_fc.as_ref().filter(|a| a.total_runs > 0).or(col.agentic.as_ref())
}

/// Build the measured inputs for `assess` from a Matrix column. Agentic metrics
/// come from the column; the hardware facts (`fits_in_vram`, `vram_pressure`) are
/// threaded in by the caller (the command computes them from the snapshot + cap;
/// the no-hardware path passes `None`/`false`). Unmeasured metrics stay
/// `None`/`NotSupported` — never a fabricated value.
pub fn from_column(
    col: &BatchColumn,
    fits_in_vram: Option<bool>,
    vram_pressure: bool,
    cliff: CliffStatus,
) -> ReadinessInputs {
    // Prefer the NATIVE aggregate when it was measured — that's the path a
    // production agent actually uses. Native present → source the gated metrics
    // from it and label the path NativeFc; otherwise the prompt-based proxy.
    let native = col.agentic_native_fc.as_ref().filter(|a| a.total_runs > 0);
    let native_fc = match native {
        Some(n) => NativeFcStatus::Tested { pass_k: n.pass_k().unwrap_or(0.0) },
        None => NativeFcStatus::NotSupported,
    };
    // Single-verdict (native-first) path label: NativeFc when native ran, else PromptBased —
    // mirrors the old `assess` derivation so this legacy builder's verdict is unchanged.
    let path = match native_fc {
        NativeFcStatus::Tested { .. } => AgentPath::NativeFc,
        NativeFcStatus::NotSupported => AgentPath::PromptBased,
    };
    let source = native_first_source(col);
    let pass_k = source.and_then(|a| a.pass_k());
    let (loops, hallucinated) = source
        .map(|a| (a.failures.infinite_loop_hits, a.failures.hallucinated_completions))
        .unwrap_or((0, 0));
    // Phase 9: per-tier strict Pass^k for the tier gate — from the SAME (native-first)
    // aggregate the other gates read, so the verdict is internally consistent.
    let tier_pass_k =
        source.map(|a| a.by_tier.iter().filter_map(|s| s.pass_k().map(|pk| (s.tier, pk))).collect()).unwrap_or_default();
    ReadinessInputs {
        pass_k,
        avg_steps: source.and_then(|a| a.avg_steps),
        ms_per_step: None, // Phase 7.4: route the StepEconomy per-step duration here
        cliff,             // context-cliff status (the command threads it in; NotProbed on the no-hardware path)
        fits_in_vram,
        vram_pressure,
        kv_downgraded: false, // legacy builder: callers thread no memory profile → f16
        loops,
        hallucinated,
        native_fc,
        path,
        tier_pass_k,
    }
}

/// The MODEL-LEVEL native function-calling capability: `Tested` when the column's native
/// pass actually ran (`total_runs > 0`), else `NotSupported`. Shared by BOTH of a column's
/// per-path rows so the `require_native_fc` gate reads native availability as a property of
/// the model, not of the row's path (a native-capable model's prompt row isn't falsely blocked).
pub fn col_native_status(col: &BatchColumn) -> NativeFcStatus {
    match col.agentic_native_fc.as_ref().filter(|a| a.total_runs > 0) {
        Some(n) => NativeFcStatus::Tested { pass_k: n.pass_k().unwrap_or(0.0) },
        None => NativeFcStatus::NotSupported,
    }
}

/// STRICT same-path aggregate for a column — the native pass for `NativeFc` (only when it
/// actually ran), the prompt pass for `PromptBased`. NO `native_first_source` fallback: a
/// row's metrics + tier ladder must come from its OWN path, never the other's.
pub fn source_for_path(col: &BatchColumn, path: AgentPath) -> Option<&AggAgentic> {
    match path {
        AgentPath::NativeFc => col.agentic_native_fc.as_ref().filter(|a| a.total_runs > 0),
        AgentPath::PromptBased => col.agentic.as_ref(),
    }
}

/// The measurement paths this column actually ran, in display order (native first, then
/// prompt). An errored/unmeasured column still yields ONE `(PromptBased, None)` entry so it
/// produces a single row (preserving the legacy single-verdict behaviour) rather than vanishing.
pub fn measured_paths(col: &BatchColumn) -> Vec<(AgentPath, Option<&AggAgentic>)> {
    let mut out = Vec::new();
    if let Some(n) = col.agentic_native_fc.as_ref().filter(|a| a.total_runs > 0) {
        out.push((AgentPath::NativeFc, Some(n)));
    }
    if let Some(p) = col.agentic.as_ref() {
        out.push((AgentPath::PromptBased, Some(p)));
    }
    if out.is_empty() {
        out.push((AgentPath::PromptBased, None));
    }
    out
}

/// Build the measured inputs for ONE path from its own aggregate `source`. Metrics
/// (`pass_k`, `avg_steps`, `loops`, `hallucinated`, `tier_pass_k`) come from `source`; the
/// model-level `native_fc` capability + the row's `path` are passed in explicitly (decoupled,
/// per `verdict.rs`). `source = None` (the unmeasured fallback) leaves the gated metrics
/// `None`/0 so the verdict blocks exactly as the legacy `from_column` does.
#[allow(clippy::too_many_arguments)]
pub fn from_source(
    source: Option<&AggAgentic>,
    path: AgentPath,
    native_fc: NativeFcStatus,
    fits_in_vram: Option<bool>,
    vram_pressure: bool,
    kv_downgraded: bool,
    cliff: CliffStatus,
) -> ReadinessInputs {
    let (loops, hallucinated) = source
        .map(|a| (a.failures.infinite_loop_hits, a.failures.hallucinated_completions))
        .unwrap_or((0, 0));
    let tier_pass_k =
        source.map(|a| a.by_tier.iter().filter_map(|s| s.pass_k().map(|pk| (s.tier, pk))).collect()).unwrap_or_default();
    ReadinessInputs {
        pass_k: source.and_then(|a| a.pass_k()),
        avg_steps: source.and_then(|a| a.avg_steps),
        ms_per_step: None,
        cliff,
        fits_in_vram,
        vram_pressure,
        kv_downgraded,
        loops,
        hallucinated,
        native_fc,
        path,
        tier_pass_k,
    }
}

/// A path's per-tier ladder merged across the same domain's tier-sibling reports — the
/// path-aware twin of `merged_by_tier_for`. Each sibling contributes ONLY its same-path
/// aggregate (`source_for_path`); a sibling that lacks this path contributes nothing, an
/// HONEST GAP in the ladder — the other path's data never bleeds in.
pub fn merged_by_tier_for_path(
    model: &str,
    backend: BackendKind,
    path: AgentPath,
    primary: &[TierStat],
    siblings: &[&BatchReport],
) -> Vec<TierStat> {
    let mut lists: Vec<&[TierStat]> = Vec::with_capacity(siblings.len() + 1);
    lists.push(primary);
    for r in siblings {
        if let Some(a) =
            r.columns.iter().find(|c| c.model == model && c.backend == backend).and_then(|c| source_for_path(c, path))
        {
            lists.push(a.by_tier.as_slice());
        }
    }
    merge_by_tier(&lists)
}

/// Every measured path's verdict for one column — the per-path emission shared by
/// `assess_readiness` (hardware-aware, with tier siblings) and `assess_report` (no
/// hardware). An errored column short-circuits to a SINGLE NotReady row carrying the real
/// run error; otherwise one `ModelVerdict` per `measured_paths` entry, each sourced strictly
/// from its own path. The model-level facts (`memory`, `cliff`, `quantization`) are computed
/// once by the caller and shared across the column's rows.
#[allow(clippy::too_many_arguments)]
pub fn verdicts_for_column(
    col: &BatchColumn,
    fits_in_vram: Option<bool>,
    vram_pressure: bool,
    cliff: CliffStatus,
    memory: Option<MemoryProfile>,
    quantization: Option<String>,
    profile: &ReadinessProfile,
    siblings: &[&BatchReport],
    think_preset: ThinkPreset,
) -> Vec<ModelVerdict> {
    if let Some(err) = &col.error {
        return vec![ModelVerdict {
            model: col.model.clone(),
            backend: col.backend,
            verdict: ReadinessVerdict {
                status: Readiness::NotReady,
                blocking: vec![format!("run error: {err}")],
                conditions: Vec::new(),
                path: AgentPath::PromptBased,
                required_tier: profile.required_tier,
                cleared_tier: None,
            },
            memory,
            avg_steps: None,
            effort: None,
            pass_k: None,
            quantization,
            cliff,
            by_tier: Vec::new(),
            failures: Default::default(),
            passes: 0,
            total_runs: 0,
            is_thinking: col.is_thinking,
            cpu_offloaded: col.cpu_offloaded,
            ctx_ceiling: col.ctx_ceiling,
            think_preset,
        }];
    }
    let native = col_native_status(col);
    // Self-describing profile → explicit advisory: a fit graded at the launch's
    // actual Q8 cache tells the verdict so (model-level, shared by both path rows).
    let kv_downgraded =
        memory.as_ref().is_some_and(|m| m.kv_precision == crate::inference::vram_math::KvPrecision::Q8);
    measured_paths(col)
        .into_iter()
        .map(|(path, source)| {
            let verdict =
                assess(&from_source(source, path, native, fits_in_vram, vram_pressure, kv_downgraded, cliff), profile);
            let (avg_steps, effort) =
                source.map(|a| (a.avg_steps, a.avg_output_tokens_success)).unwrap_or((None, None));
            let primary = source.map(|a| a.by_tier.clone()).unwrap_or_default();
            let by_tier = merged_by_tier_for_path(&col.model, col.backend, path, &primary, siblings);
            ModelVerdict {
                model: col.model.clone(),
                backend: col.backend,
                verdict,
                memory: memory.clone(),
                avg_steps,
                effort,
                pass_k: source.and_then(|a| a.pass_k()),
                quantization: quantization.clone(),
                cliff,
                by_tier,
                failures: source.map(|a| a.failures.clone()).unwrap_or_default(),
                passes: source.map(|a| a.passes).unwrap_or(0),
                total_runs: source.map(|a| a.total_runs).unwrap_or(0),
                is_thinking: col.is_thinking,
                cpu_offloaded: col.cpu_offloaded,
                ctx_ceiling: col.ctx_ceiling,
                think_preset,
            }
        })
        .collect()
}

/// One column's verdict against a profile, with the hardware facts + measured cliff
/// threaded in. An errored column short-circuits to NotReady carrying the real run
/// error — we never synthesize a score for a column that failed to produce data.
/// The single home for the error/assess branch, shared by `assess_report` (no
/// hardware/cliff) and the hardware-aware command.
pub fn verdict_for(
    col: &BatchColumn,
    fits_in_vram: Option<bool>,
    vram_pressure: bool,
    cliff: CliffStatus,
    profile: &ReadinessProfile,
) -> ReadinessVerdict {
    match &col.error {
        Some(err) => ReadinessVerdict {
            status: Readiness::NotReady,
            blocking: vec![format!("run error: {err}")],
            conditions: Vec::new(),
            path: AgentPath::PromptBased,
            required_tier: profile.required_tier,
            cleared_tier: None, // an errored column measured nothing
        },
        None => assess(&from_column(col, fits_in_vram, vram_pressure, cliff), profile),
    }
}

/// Efficiency telemetry (avg_steps, effort) for the recommender ranking —
/// **native-first**, the exact aggregate the verdict gates on, so the recommender
/// praises the same telemetry the verdict was computed from (not the wrong column).
pub fn agentic_metrics(col: &BatchColumn) -> (Option<f64>, Option<f64>) {
    match native_first_source(col) {
        Some(a) => (a.avg_steps, a.avg_output_tokens_success),
        None => (None, None),
    }
}

/// The measured Pass^k the verdict gated on — **native-first**, as a raw fraction.
/// `None` when no agentic run produced data (the row then renders "N/A").
pub fn pass_k_of(col: &BatchColumn) -> Option<f64> {
    native_first_source(col).and_then(|a| a.pass_k())
}

/// Resolve a column's real quantization: the Ollama installed-models registry first,
/// else parse it from the model name (a GGUF / llama.cpp / MLX model, or Ollama
/// offline). NEVER fabricated — it's the quant the name actually encodes, the same one
/// the VerdictTable shows — so a publish row (which requires a quant) isn't dropped just
/// because the registry was unavailable. `None` only when neither source knows it.
pub fn resolve_quant(registry_hit: Option<String>, model: &str) -> Option<String> {
    registry_hit.or_else(|| quant_from_filename(model))
}

/// Union several per-tier ladders into one, keyed by `tier` (FIRST occurrence wins —
/// pass the authoritative list first), sorted ascending. Each built-in tier lives in
/// exactly one collection, so this is a pure union with NO per-tier re-averaging:
/// `avg_steps` and per-tier `failures` carry through verbatim. Powers the Agent
/// Report's per-domain Tier Progression Matrix, which accumulates across a domain's
/// tier-sibling collections.
pub fn merge_by_tier(lists: &[&[TierStat]]) -> Vec<TierStat> {
    let mut out: Vec<TierStat> = Vec::new();
    for list in lists {
        for ts in *list {
            if !out.iter().any(|e| e.tier == ts.tier) {
                out.push(ts.clone());
            }
        }
    }
    out.sort_by_key(|ts| ts.tier);
    out
}

/// A model's per-tier ladder merged across the same domain's tier-sibling reports.
/// `primary` (the selected collection's own native-first `by_tier`) wins on tier
/// collision; each sibling contributes ONLY its matching `(model, backend)` column —
/// so a different model's tiers are never pulled into this model's ladder (no
/// cross-model "Frankenstein ladder"). `(model, backend)` is the load-bearing key.
pub fn merged_by_tier_for(
    model: &str,
    backend: BackendKind,
    primary: &[TierStat],
    siblings: &[&BatchReport],
) -> Vec<TierStat> {
    let mut lists: Vec<&[TierStat]> = Vec::with_capacity(siblings.len() + 1);
    lists.push(primary);
    for r in siblings {
        if let Some(a) = r
            .columns
            .iter()
            .find(|c| c.model == model && c.backend == backend)
            .and_then(native_first_source)
        {
            lists.push(a.by_tier.as_slice());
        }
    }
    merge_by_tier(&lists)
}

/// Assess every model in a persisted batch report against a profile, with no
/// hardware facts (VRAM unmeasured). The CLI-without-cap and pure-test path. Emits one
/// verdict PER MEASURED PATH (native + prompt) per column, like the hardware-aware command —
/// no hardware/cliff/quant and no tier siblings on this path.
pub fn assess_report(report: &BatchReport, profile: &ReadinessProfile) -> Vec<ModelVerdict> {
    report
        .columns
        .iter()
        .flat_map(|col| {
            // No hardware path: VRAM unmeasured, cliff NotProbed, no quant registry, no siblings.
            verdicts_for_column(col, None, false, CliffStatus::NotProbed, None, None, profile, &[], report.think_preset.unwrap_or_default())
        })
        .collect()
}

#[cfg(test)]
#[path = "inputs_tests.rs"]
mod tests;
