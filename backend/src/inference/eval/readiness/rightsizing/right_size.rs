use crate::inference::eval::readiness::types::{ModelVerdict, Readiness};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One side of a right-sizing comparison — the measured facts of a single
/// assessed variant. Percent-only feature: no cost or currency fields, ever.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RightSizingPick {
    pub model: String,
    pub quantization: Option<String>,
    pub weights_bytes: u64,
    /// Measured weights+KV total (from the variant's `MemoryProfile`); `None`
    /// when the fit was unmeasured — never a guess.
    pub total_bytes: Option<u64>,
    pub pass_k: Option<f64>,
    pub status: Readiness,
}

/// The right-sizing verdict for one model family at one parameter size:
/// baseline (largest weights) vs pick (smallest that is still Ready — the
/// product's core promise), with measured percent reductions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RightSizingGroup {
    /// "family parameter_size" — the grouping key, human-readable.
    pub group: String,
    pub baseline: RightSizingPick,
    pub pick: RightSizingPick,
    /// (1 − pick.weights / baseline.weights) × 100 — always measurable (exact
    /// on-disk sizes).
    pub size_reduction_pct: f64,
    /// (1 − pick.total / baseline.total) × 100 — only when BOTH totals were
    /// measured AND graded at the same KV precision; else `None` (a
    /// mixed-precision percentage would be a fabricated comparison).
    pub memory_reduction_pct: Option<f64>,
    /// (pick.pass_k − baseline.pass_k) × 100, signed percentage points; only
    /// when both were measured.
    pub quality_delta_pp: Option<f64>,
    /// The pick is Conditional (no Ready variant was small enough) — the UI
    /// surfaces its conditions instead of presenting it as a clean win.
    pub pick_is_conditional: bool,
    pub rationale: String,
}

/// Per-model metadata the verdicts don't carry: `(family parameter_size, weights_bytes)`
/// from the installed-model registry. Models absent from the map are skipped
/// (no grouping key → no honest comparison).
pub type ModelMeta = HashMap<String, (String, u64)>;

/// Right-size every family with ≥2 assessed variants: baseline = the largest
/// weights, pick = the smallest weights that is still `Ready` (else the smallest
/// `Conditional`, flagged). Returns the groups plus a hint when nothing was
/// comparable ("assess ≥2 quants…"). Pure over ranked verdicts (best-first —
/// `recommend::rank` order); the first row per model wins the dedupe.
pub fn summarize(verdicts: &[ModelVerdict], meta: &ModelMeta) -> (Vec<RightSizingGroup>, Option<String>) {
    // Dedupe to one representative row per model (verdicts arrive best-first).
    let mut seen: Vec<&str> = Vec::new();
    let mut groups: HashMap<&str, Vec<&ModelVerdict>> = HashMap::new();
    for v in verdicts {
        if seen.contains(&v.model.as_str()) {
            continue;
        }
        seen.push(&v.model);
        let Some((group, weights)) = meta.get(&v.model) else { continue };
        if *weights == 0 {
            continue; // a zero-byte registry row is a broken input, not a baseline
        }
        groups.entry(group.as_str()).or_default().push(v);
    }

    let mut out: Vec<RightSizingGroup> = Vec::new();
    for (group, members) in groups {
        if members.len() < 2 {
            continue; // one variant compares against nothing
        }
        let weight_of = |v: &ModelVerdict| meta.get(&v.model).map(|(_, w)| *w).unwrap_or(0);
        let baseline_v = members.iter().max_by_key(|v| weight_of(v)).copied().unwrap_or(members[0]);
        // Pick the SMALLEST USABLE variant — Ready or Conditional — so the
        // right-sizing signal is as strong as the hardware honestly allows; a
        // Conditional pick is FLAGGED (and its conditions surfaced by the UI), never
        // hidden. A group with nothing usable (all NotReady) yields no card.
        let Some(pick_v) = members
            .iter()
            .filter(|v| matches!(v.verdict.status, Readiness::Ready | Readiness::Conditional))
            .min_by_key(|v| weight_of(v))
            .copied()
        else {
            continue;
        };
        let pick_is_conditional = pick_v.verdict.status == Readiness::Conditional;

        let to_pick = |v: &ModelVerdict| RightSizingPick {
            model: v.model.clone(),
            quantization: v.quantization.clone(),
            weights_bytes: weight_of(v),
            total_bytes: v.memory.as_ref().map(|m| m.total_bytes),
            pass_k: v.pass_k,
            status: v.verdict.status,
        };
        let baseline = to_pick(baseline_v);
        let pick = to_pick(pick_v);

        if pick.model == baseline.model {
            out.push(RightSizingGroup {
                group: group.to_string(),
                size_reduction_pct: 0.0,
                memory_reduction_pct: None,
                quality_delta_pp: None,
                pick_is_conditional,
                rationale: "already the smallest variant that clears the bar".into(),
                baseline,
                pick,
            });
            continue;
        }

        let size_reduction_pct = (1.0 - pick.weights_bytes as f64 / baseline.weights_bytes as f64) * 100.0;
        // Memory % only when both totals are measured AND graded at the same KV
        // precision — a Q8-graded total vs an f16-graded one is not a comparison.
        let same_precision = match (&baseline_v.memory, &pick_v.memory) {
            (Some(b), Some(p)) => b.kv_precision == p.kv_precision,
            _ => false,
        };
        let memory_reduction_pct = match (baseline.total_bytes, pick.total_bytes) {
            (Some(b), Some(p)) if same_precision && b > 0 => Some((1.0 - p as f64 / b as f64) * 100.0),
            _ => None,
        };
        let quality_delta_pp = match (pick.pass_k, baseline.pass_k) {
            (Some(p), Some(b)) => Some((p - b) * 100.0),
            _ => None,
        };
        let precision_note = match (&baseline_v.memory, &pick_v.memory) {
            (Some(b), Some(p)) if b.kv_precision != p.kv_precision => {
                format!(" (memory graded at different KV precisions: {:?} vs {:?} — % omitted)", b.kv_precision, p.kv_precision)
            }
            _ => String::new(),
        };
        let rationale = format!(
            "smallest {} variant on this hardware{precision_note}",
            if pick_is_conditional { "Conditional" } else { "Ready" }
        );
        out.push(RightSizingGroup {
            group: group.to_string(),
            baseline,
            pick,
            size_reduction_pct,
            memory_reduction_pct,
            quality_delta_pp,
            pick_is_conditional,
            rationale,
        });
    }
    out.sort_by(|a, b| a.group.cmp(&b.group)); // deterministic order for the UI + exports
    let hint = out
        .is_empty()
        .then(|| "Assess ≥2 quants of the same family (same parameter size) to compare right-sizing.".to_string());
    (out, hint)
}

#[cfg(test)]
#[path = "right_size_tests.rs"]
mod tests;
