use super::*;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::eval::readiness::types::{AgentPath, CliffStatus, ReadinessVerdict};
use crate::inference::eval::readiness::vram_fit::MemoryProfile;
use crate::inference::vram_math::KvPrecision;

const GIB: u64 = 1_073_741_824;

/// A verdict fixture carrying only the fields `summarize` reads: status, pass_k,
/// quantization, and the memory total/precision. `weights` is supplied via the
/// `ModelMeta` map (it comes from the installed registry, not the verdict).
fn verdict(model: &str, status: Readiness, pass_k: Option<f64>, total: Option<(u64, KvPrecision)>) -> ModelVerdict {
    ModelVerdict {
        model: model.into(),
        backend: BackendKind::Ollama,
        verdict: ReadinessVerdict {
            status,
            blocking: vec![],
            conditions: vec![],
            path: AgentPath::PromptBased,
            required_tier: Default::default(),
            cleared_tier: None,
        },
        memory: total.map(|(total_bytes, kv_precision)| MemoryProfile {
            weights_bytes: 0,
            kv_cache_bytes: 0,
            total_bytes,
            cap_bytes: 64 * GIB,
            context_length: 8192,
            fits: true,
            pressure: false,
            estimated: false,
            kv_precision,
        }),
        avg_steps: None,
        effort: None,
        pass_k,
        quantization: Some(model.split('-').next_back().unwrap_or("").to_string()),
        cliff: CliffStatus::NotProbed,
        by_tier: Vec::new(),
        failures: Default::default(),
        passes: 0,
        total_runs: 0,
        is_thinking: false,
        cpu_offloaded: false,
        ctx_ceiling: None,
        think_preset: Default::default(),
    }
}

fn meta(pairs: &[(&str, &str, u64)]) -> ModelMeta {
    pairs.iter().map(|(m, g, w)| (m.to_string(), (g.to_string(), *w))).collect()
}

#[test]
fn picks_smallest_ready_variant_and_computes_percent_reductions() {
    // q8 baseline 9 GiB, q4 pick 5 GiB; pass_k 0.90 → 0.85; totals same (f16) precision.
    let verdicts = vec![
        verdict("qwen-q8", Readiness::Ready, Some(0.90), Some((9 * GIB, KvPrecision::F16))),
        verdict("qwen-q4", Readiness::Ready, Some(0.85), Some((5 * GIB, KvPrecision::F16))),
    ];
    let m = meta(&[("qwen-q8", "qwen 9B", 9 * GIB), ("qwen-q4", "qwen 9B", 5 * GIB)]);
    let (groups, hint) = summarize(&verdicts, &m);
    assert!(hint.is_none());
    assert_eq!(groups.len(), 1);
    let g = &groups[0];
    assert_eq!(g.baseline.model, "qwen-q8");
    assert_eq!(g.pick.model, "qwen-q4");
    assert!(!g.pick_is_conditional);
    // 1 - 5/9 = 44.44%
    assert!((g.size_reduction_pct - 44.444).abs() < 0.01, "{}", g.size_reduction_pct);
    // 1 - 5/9 = 44.44% (same-precision totals)
    assert!((g.memory_reduction_pct.unwrap() - 44.444).abs() < 0.01);
    // 0.85 - 0.90 = -5.0 pp
    assert!((g.quality_delta_pp.unwrap() - (-5.0)).abs() < 0.001);
}

#[test]
fn falls_back_to_conditional_pick_when_no_ready_is_small_enough() {
    let verdicts = vec![
        verdict("m-q8", Readiness::Ready, Some(0.9), Some((9 * GIB, KvPrecision::F16))),
        verdict("m-q4", Readiness::Conditional, Some(0.8), Some((5 * GIB, KvPrecision::F16))),
    ];
    let m = meta(&[("m-q8", "fam 9B", 9 * GIB), ("m-q4", "fam 9B", 5 * GIB)]);
    let (groups, _) = summarize(&verdicts, &m);
    assert_eq!(groups[0].pick.model, "m-q4");
    assert!(groups[0].pick_is_conditional, "no Ready was small enough → the smaller Conditional, flagged");
    assert!(groups[0].rationale.contains("Conditional"));
}

#[test]
fn single_variant_group_yields_hint_not_a_card() {
    let verdicts = vec![verdict("solo", Readiness::Ready, Some(0.9), Some((5 * GIB, KvPrecision::F16)))];
    let m = meta(&[("solo", "fam 9B", 5 * GIB)]);
    let (groups, hint) = summarize(&verdicts, &m);
    assert!(groups.is_empty());
    assert!(hint.unwrap().contains("≥2 quants"));
}

#[test]
fn unmeasured_memory_yields_no_memory_pct_but_still_a_size_pct() {
    // Sizes are always on-disk exact; totals absent → memory % is None, never a guess.
    let verdicts = vec![
        verdict("x-q8", Readiness::Ready, Some(0.9), None),
        verdict("x-q4", Readiness::Ready, Some(0.9), None),
    ];
    let m = meta(&[("x-q8", "fam 9B", 9 * GIB), ("x-q4", "fam 9B", 5 * GIB)]);
    let (groups, _) = summarize(&verdicts, &m);
    assert!(groups[0].size_reduction_pct > 0.0);
    assert!(groups[0].memory_reduction_pct.is_none());
}

#[test]
fn mixed_kv_precision_omits_memory_pct_with_a_note() {
    // Baseline f16 vs pick Q8 (llama.cpp graded under pressure) → not comparable.
    let verdicts = vec![
        verdict("y-q8", Readiness::Ready, Some(0.9), Some((9 * GIB, KvPrecision::F16))),
        verdict("y-q4", Readiness::Ready, Some(0.9), Some((5 * GIB, KvPrecision::Q8))),
    ];
    let m = meta(&[("y-q8", "fam 9B", 9 * GIB), ("y-q4", "fam 9B", 5 * GIB)]);
    let (groups, _) = summarize(&verdicts, &m);
    assert!(groups[0].memory_reduction_pct.is_none(), "mixed precision → no fabricated %");
    assert!(groups[0].rationale.contains("KV precision"), "the note explains why: {}", groups[0].rationale);
}

#[test]
fn dedupes_per_model_keeping_the_best_ranked_row() {
    // Two rows for the same model (native + prompt paths, best-first) must count once.
    let verdicts = vec![
        verdict("d-q8", Readiness::Ready, Some(0.9), Some((9 * GIB, KvPrecision::F16))),
        verdict("d-q8", Readiness::Conditional, Some(0.5), Some((9 * GIB, KvPrecision::F16))),
        verdict("d-q4", Readiness::Ready, Some(0.85), Some((5 * GIB, KvPrecision::F16))),
    ];
    let m = meta(&[("d-q8", "fam 9B", 9 * GIB), ("d-q4", "fam 9B", 5 * GIB)]);
    let (groups, _) = summarize(&verdicts, &m);
    assert_eq!(groups.len(), 1, "the duplicate d-q8 row must not create a phantom group member");
    assert_eq!(groups[0].baseline.status, Readiness::Ready, "the FIRST (best-ranked) d-q8 row won");
}

#[test]
fn zero_weight_registry_row_is_skipped_never_panics() {
    let verdicts = vec![
        verdict("z-a", Readiness::Ready, Some(0.9), None),
        verdict("z-b", Readiness::Ready, Some(0.9), None),
    ];
    // z-a has a broken 0-byte size → dropped, leaving one variant → hint, no divide-by-zero.
    let m = meta(&[("z-a", "fam 9B", 0), ("z-b", "fam 9B", 5 * GIB)]);
    let (groups, hint) = summarize(&verdicts, &m);
    assert!(groups.is_empty());
    assert!(hint.is_some());
}

#[test]
fn all_not_ready_group_yields_no_card() {
    let verdicts = vec![
        verdict("n-q8", Readiness::NotReady, Some(0.2), Some((9 * GIB, KvPrecision::F16))),
        verdict("n-q4", Readiness::NotReady, Some(0.1), Some((5 * GIB, KvPrecision::F16))),
    ];
    let m = meta(&[("n-q8", "fam 9B", 9 * GIB), ("n-q4", "fam 9B", 5 * GIB)]);
    let (groups, hint) = summarize(&verdicts, &m);
    assert!(groups.is_empty(), "nothing safe to recommend → no card");
    assert!(hint.is_some());
}

#[test]
fn baseline_equals_pick_when_the_largest_is_also_the_only_ready() {
    // Largest (q8) is the only Ready → pick == baseline → "already smallest" rationale.
    let verdicts = vec![
        verdict("s-q8", Readiness::Ready, Some(0.9), Some((9 * GIB, KvPrecision::F16))),
        verdict("s-q4", Readiness::NotReady, Some(0.3), Some((5 * GIB, KvPrecision::F16))),
    ];
    let m = meta(&[("s-q8", "fam 9B", 9 * GIB), ("s-q4", "fam 9B", 5 * GIB)]);
    let (groups, _) = summarize(&verdicts, &m);
    assert_eq!(groups[0].pick.model, "s-q8");
    assert_eq!(groups[0].baseline.model, "s-q8");
    assert_eq!(groups[0].size_reduction_pct, 0.0);
    assert!(groups[0].rationale.contains("already"));
}
