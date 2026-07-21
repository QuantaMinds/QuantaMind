//! Per-task run costs for the CLI — the Rust mirror of the app's `taskCost` derivation
//! (frontend/src/features/eval/state/taskCost.ts), same honesty rules: a field is `None`
//! when NO step reported it ("Not available", never a fabricated 0); the thinking count
//! carries its provenance flag; RSS is a max of step-END samples, never a true peak;
//! native and prompt passes are separate rows (different eval methods — never blended).

use crate::inference::eval::agentic::step::TrajectoryStep;
use crate::inference::eval::batch::{BatchColumn, TaskOutcome};
use crate::inference::vram_math::{kv_cache_bytes_at, KvPrecision};
use serde::Serialize;
use std::collections::BTreeMap;

/// One (task, pass) cost row. Field names carry the semantics on purpose —
/// `max_step_end_rss_bytes` IS the measurement's honest name.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct TaskCostRow {
    pub task_id: String,
    /// "native" (the model's tool API) or "prompt" — kept separate, never blended.
    pub pass: String,
    pub runs: u32,
    pub steps: u32,
    pub prefill_ms_total: Option<u64>,
    pub eval_ms_total: Option<u64>,
    pub output_tokens_total: Option<u64>,
    pub reasoning_tokens_total: Option<u64>,
    /// True only when the thinking count is a MEASURED channel split (llama.cpp
    /// /tokenize); false ⇒ the backend's combined count → render "(no split)".
    pub thinking_split_measured: bool,
    /// Measured prefix-cache reuse (llama.cpp `cache_n` only; Ollama reports none).
    pub cache_hit_tokens_total: Option<u64>,
    /// Max single-run token occupancy — sizes the KV figure. Sums above can exceed it:
    /// they accumulate across runs; this is one moment.
    pub peak_context_tokens: Option<u32>,
    pub context_window: Option<u32>,
    pub max_step_end_rss_bytes: Option<u64>,
    /// Measured wall clock of the whole Pass^k batch (model + world time).
    pub wall_ms: Option<u64>,
    /// The task died of a host out-of-memory (classified once, in Rust).
    pub oom: bool,
}

/// KV bytes for the run's peak occupancy at each cache precision — the same canonical
/// formula the launch planner uses. `None` when the model's dims are unmeasurable.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct KvAtPeak {
    pub peak_tokens: u32,
    pub f16_bytes: u64,
    pub q8_0_bytes: u64,
    pub q4_0_bytes: u64,
    /// The model didn't report its KV-head count — the figures are conservative
    /// OVER-estimates (they under-promise, never over-promise).
    pub conservative: bool,
}

/// The memory facts of one model's run, with provenance strings the consumer can print
/// verbatim — the same ladder the app's Latency view shows.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct MemoryFacts {
    /// Ollama: resident size from /api/ps (weights + context reservation). llama.cpp:
    /// the GGUF size at launch when the app stamped it. `None` = not measurable.
    pub model_bytes: Option<u64>,
    pub model_bytes_provenance: &'static str,
    pub offload_bytes: Option<u64>,
    pub quantization_claimed: Option<String>,
    /// "f16" | "q8_0" from an app-launched llama-server; `None` = unreported (Ollama,
    /// or an externally managed server — never guessed).
    pub kv_cache_type: Option<String>,
    pub kv_at_peak: Option<KvAtPeak>,
}

/// Everything `--costs` reports for one model's run.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct RunCosts {
    pub model: String,
    pub tasks: Vec<TaskCostRow>,
    pub memory: MemoryFacts,
}

fn sum_reported(steps: &[TrajectoryStep], pick: impl Fn(&TrajectoryStep) -> Option<u64>) -> Option<u64> {
    let mut saw = false;
    let mut total = 0u64;
    for s in steps {
        if let Some(v) = pick(s) {
            saw = true;
            total += v;
        }
    }
    saw.then_some(total)
}

/// A step's token occupancy at turn end (mirrors taskCost.ts `occupancy`).
fn occupancy(s: &TrajectoryStep) -> Option<u32> {
    if let Some(u) = s.context_used {
        return Some(u);
    }
    if s.cache_n.is_none() && s.prefill_tokens.is_none() {
        return None;
    }
    Some(s.cache_n.unwrap_or(0) + s.prefill_tokens.unwrap_or(0) + s.output_tokens.unwrap_or(0))
}

/// Fold one (task, pass) cell's steps + outcome into a cost row.
pub fn task_cost_row(task_id: &str, native: bool, steps: &[TrajectoryStep], outcome: Option<&TaskOutcome>) -> TaskCostRow {
    let runs = steps.iter().map(|s| s.run_index).collect::<std::collections::HashSet<_>>().len() as u32;
    let peak = steps.iter().filter_map(occupancy).max();
    let (wall_ms, oom) = match outcome {
        Some(TaskOutcome::Agentic { report }) => (report.wall_ms, false),
        Some(TaskOutcome::Error { oom, .. }) => (None, *oom),
        _ => (None, false),
    };
    TaskCostRow {
        task_id: task_id.to_string(),
        pass: if native { "native" } else { "prompt" }.to_string(),
        runs,
        steps: steps.len() as u32,
        prefill_ms_total: sum_reported(steps, |s| s.prefill_ms),
        eval_ms_total: sum_reported(steps, |s| s.eval_ms),
        output_tokens_total: sum_reported(steps, |s| s.output_tokens.map(u64::from)),
        reasoning_tokens_total: sum_reported(steps, |s| s.reasoning_tokens.map(u64::from)),
        thinking_split_measured: steps.iter().any(|s| s.thinking_split_measured),
        cache_hit_tokens_total: sum_reported(steps, |s| s.cache_n.map(u64::from)),
        peak_context_tokens: peak,
        context_window: steps.iter().find_map(|s| s.context_window),
        max_step_end_rss_bytes: steps.iter().filter_map(|s| s.resident_bytes).max(),
        wall_ms,
        oom,
    }
}

/// KV bytes at the run's overall peak, from model dims — all three precisions via the
/// canonical formula. `dims = None` (unmeasurable) → `None`, never a guess.
pub fn kv_at_peak(
    dims: Option<(u64, u64, u64, u64, bool)>, // (layers, head_count, head_count_kv, embedding_length, conservative)
    peak_tokens: Option<u32>,
) -> Option<KvAtPeak> {
    let (layers, heads, kv_heads, embed, conservative) = dims?;
    let peak = peak_tokens?;
    if peak == 0 {
        return None;
    }
    let at = |p: KvPrecision| kv_cache_bytes_at(p, layers, heads, kv_heads, embed, u64::from(peak));
    Some(KvAtPeak {
        peak_tokens: peak,
        f16_bytes: at(KvPrecision::F16),
        q8_0_bytes: at(KvPrecision::Q8),
        q4_0_bytes: at(KvPrecision::Q4),
        conservative,
    })
}

/// Memory facts off the stamped report column (the SHARED `run_facts` stamping ran
/// before this) + the KV-at-peak figure.
pub fn memory_facts(column: Option<&BatchColumn>, kv: Option<KvAtPeak>) -> MemoryFacts {
    let vram = column.and_then(|c| c.weights_vram_bytes);
    let total = column.and_then(|c| c.weights_total_bytes);
    MemoryFacts {
        model_bytes: vram.or(total),
        model_bytes_provenance: if vram.is_some() {
            "measured (/api/ps size_vram — weights + the context buffer reserved at load)"
        } else if total.is_some() {
            "measured (GGUF size at launch — llama.cpp reports no resident split)"
        } else {
            "not measurable (no placement probe answered)"
        },
        offload_bytes: column.and_then(|c| if c.cpu_offloaded { c.offload_bytes } else { None }),
        quantization_claimed: column.and_then(|c| c.quantization_claimed.clone()),
        kv_cache_type: column.and_then(|c| c.kv_cache_type.clone()),
        kv_at_peak: kv,
    }
}

/// Assemble the full `RunCosts` from captured (task, pass) cells, sorted for stable output.
pub fn assemble(
    model: &str,
    cells: &BTreeMap<(String, bool), Vec<TrajectoryStep>>,
    outcomes: &BTreeMap<(String, bool), TaskOutcome>,
    column: Option<&BatchColumn>,
    dims: Option<(u64, u64, u64, u64, bool)>,
) -> RunCosts {
    let tasks: Vec<TaskCostRow> = cells
        .iter()
        .map(|((task, native), steps)| task_cost_row(task, *native, steps, outcomes.get(&(task.clone(), *native))))
        .collect();
    let peak = tasks.iter().filter_map(|t| t.peak_context_tokens).max();
    RunCosts {
        model: model.to_string(),
        memory: memory_facts(column, kv_at_peak(dims, peak)),
        tasks,
    }
}

#[cfg(test)]
#[path = "costs_tests.rs"]
mod tests;
