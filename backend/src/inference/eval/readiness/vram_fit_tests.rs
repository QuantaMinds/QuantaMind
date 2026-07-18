use super::{estimate, try_profile, Dims, PRESSURE_FRACTION};
use crate::inference::vram_math::KvPrecision;

/// Cross-language drift guard: the frontend mirrors `PRESSURE_FRACTION` in
/// `frontend/src/shared/memory/pressure.ts` (a compile-time constant; an IPC
/// round-trip for one float buys nothing). This test reads that file and pins the
/// two values together, so they can never silently diverge — the exact failure
/// `fit.ts` already exhibits (its unrelated TIGHT_FRACTION drifted to 0.7 while a
/// stale doc comment here claimed to mirror it). Modeled on the test-enforced
/// invariant precedent in `persistence/publish/row_tests.rs`.
#[test]
fn frontend_pressure_constant_matches_backend() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../frontend/src/shared/memory/pressure.ts");
    let src = std::fs::read_to_string(path).expect("frontend/src/shared/memory/pressure.ts must exist");
    let line = src
        .lines()
        .find(|l| l.contains("PRESSURE_FRACTION =") && !l.trim_start().starts_with("//"))
        .expect("pressure.ts must define PRESSURE_FRACTION");
    let value: f64 = line
        .split('=')
        .nth(1)
        .and_then(|v| v.trim().trim_end_matches(';').parse().ok())
        .expect("PRESSURE_FRACTION must be a bare float literal");
    assert_eq!(
        value, PRESSURE_FRACTION,
        "frontend pressure.ts PRESSURE_FRACTION ({value}) drifted from backend vram_fit.rs ({PRESSURE_FRACTION})"
    );
}

const GIB: u64 = 1_073_741_824;

fn dims() -> Dims {
    Dims { layers: 32, head_count: 32, head_count_kv: 8, embedding_length: 4096, context_length: 8192, kv_estimated: false }
}

#[test]
fn try_profile_is_none_when_any_input_is_missing() {
    assert!(try_profile(None, Some(dims()), Some(8192), Some(16 * GIB), KvPrecision::F16).is_none()); // no weights
    assert!(try_profile(Some(5 * GIB), None, Some(8192), Some(16 * GIB), KvPrecision::F16).is_none()); // no dims
    assert!(try_profile(Some(5 * GIB), Some(dims()), Some(8192), None, KvPrecision::F16).is_none()); // no cap → not measured
}

#[test]
fn fallback_context_is_capped_at_8k_not_the_model_max() {
    // No run num_ctx + a 262 k-context model → estimate at 8 k (DEFAULT_FALLBACK_CTX),
    // NOT 262 k (which would balloon the cache to ~100 GB).
    let big = Dims { context_length: 262_144, ..dims() };
    let p = try_profile(Some(5 * GIB), Some(big), None, Some(16 * GIB), KvPrecision::F16).unwrap();
    assert_eq!(p.context_length, super::DEFAULT_FALLBACK_CTX);
    assert_eq!(p.kv_cache_bytes, GIB); // 8 k → exactly 1 GiB for these dims
}

#[test]
fn explicit_run_num_ctx_is_honored_even_above_the_cap() {
    let big = Dims { context_length: 262_144, ..dims() };
    let p = try_profile(Some(5 * GIB), Some(big), Some(32_768), Some(64 * GIB), KvPrecision::F16).unwrap();
    assert_eq!(p.context_length, 32_768); // the run asked for it → estimate what they ran
}

#[test]
fn fallback_uses_model_max_when_it_is_below_the_cap() {
    let small = Dims { context_length: 4096, ..dims() };
    let p = try_profile(Some(5 * GIB), Some(small), None, Some(16 * GIB), KvPrecision::F16).unwrap();
    assert_eq!(p.context_length, 4096); // min(4096, 8192)
}

#[test]
fn try_profile_propagates_the_estimated_flag_from_dims() {
    let measured = try_profile(Some(5 * GIB), Some(dims()), Some(8192), Some(16 * GIB), KvPrecision::F16).unwrap();
    assert!(!measured.estimated); // real KV head count → exact
    let est = try_profile(Some(5 * GIB), Some(Dims { kv_estimated: true, ..dims() }), Some(8192), Some(16 * GIB), KvPrecision::F16).unwrap();
    assert!(est.estimated); // defaulted KV head count → conservative estimate
}
// Llama-3-8B (GQA): 32 layers, 32 heads, 8 KV heads, 4096 emb → KV @ 8k = exactly 1 GiB.
fn llama3_8b(weights: u64, ctx: u32, cap: u64) -> super::MemoryProfile {
    estimate(weights, 32, 32, 8, 4096, ctx, cap, KvPrecision::F16)
}

#[test]
fn kv_cache_uses_the_canonical_formula() {
    let p = llama3_8b(5 * GIB, 8192, 16 * GIB);
    assert_eq!(p.kv_cache_bytes, GIB); // 1 GiB at 8k context
    assert_eq!(p.total_bytes, 6 * GIB); // 5 GiB weights + 1 GiB cache
    assert_eq!(p.context_length, 8192);
}

#[test]
fn fits_comfortably_below_the_cap() {
    let p = llama3_8b(5 * GIB, 8192, 16 * GIB); // 6 GiB total vs 16 GiB cap
    assert!(p.fits);
    assert!(!p.pressure); // 6/16 = 37%, well under the 85% band
}

#[test]
fn fits_but_flags_pressure_near_the_ceiling() {
    // 6 GiB total vs a 6.5 GiB cap → 92% of cap → fits with pressure.
    let cap = 6 * GIB + GIB / 2;
    let p = llama3_8b(5 * GIB, 8192, cap);
    assert!(p.fits);
    assert!(p.pressure);
}

#[test]
fn does_not_fit_when_total_exceeds_cap() {
    let p = llama3_8b(5 * GIB, 8192, 5 * GIB); // 6 GiB total vs 5 GiB cap
    assert!(!p.fits);
    assert!(!p.pressure); // pressure only meaningful when it fits
}

#[test]
fn larger_context_grows_the_cache_and_can_tip_the_fit() {
    let small = llama3_8b(5 * GIB, 8192, 7 * GIB); // 6 GiB total → fits
    let large = llama3_8b(5 * GIB, 16384, 7 * GIB); // 5 + 2 GiB cache = 7 GiB → still fits exactly
    assert!(small.fits);
    assert_eq!(large.kv_cache_bytes, 2 * GIB);
    assert!(large.fits); // 7 GiB total == 7 GiB cap (≤)
    let tighter = llama3_8b(5 * GIB, 16384, 7 * GIB - 1);
    assert!(!tighter.fits); // one byte over → won't fit
}

#[test]
fn try_profile_at_f16_is_byte_identical_to_legacy() {
    // The exact numbers this suite pinned BEFORE the precision parameter existed —
    // the tripwire that threading `KvPrecision::F16` through changed nothing.
    let p = try_profile(Some(5 * GIB), Some(dims()), Some(8192), Some(16 * GIB), KvPrecision::F16).unwrap();
    assert_eq!(p.weights_bytes, 5 * GIB);
    assert_eq!(p.kv_cache_bytes, GIB);
    assert_eq!(p.total_bytes, 6 * GIB);
    assert_eq!(p.cap_bytes, 16 * GIB);
    assert_eq!(p.context_length, 8192);
    assert!(p.fits && !p.pressure && !p.estimated);
    assert_eq!(p.kv_precision, KvPrecision::F16);
}

#[test]
fn try_profile_at_q8_halves_only_the_cache() {
    let f16 = try_profile(Some(5 * GIB), Some(dims()), Some(8192), Some(16 * GIB), KvPrecision::F16).unwrap();
    let q8 = try_profile(Some(5 * GIB), Some(dims()), Some(8192), Some(16 * GIB), KvPrecision::Q8).unwrap();
    assert_eq!(q8.weights_bytes, f16.weights_bytes, "weights are never scaled");
    assert_eq!(q8.kv_cache_bytes, f16.kv_cache_bytes / 2);
    assert_eq!(q8.total_bytes, 5 * GIB + GIB / 2);
    assert_eq!(q8.kv_precision, KvPrecision::Q8, "the profile says what it was graded at");
}

#[test]
fn a_q8_grading_can_fit_what_f16_cannot() {
    // 5 GiB weights + 1 GiB f16 cache vs a 5.75 GiB cap: f16 doesn't fit; the
    // same model graded at a Q8 cache (½ GiB) does — the gate-at-actual-KV case.
    let cap = 5 * GIB + GIB / 2 + GIB / 4;
    let f16 = try_profile(Some(5 * GIB), Some(dims()), Some(8192), Some(cap), KvPrecision::F16).unwrap();
    let q8 = try_profile(Some(5 * GIB), Some(dims()), Some(8192), Some(cap), KvPrecision::Q8).unwrap();
    assert!(!f16.fits);
    assert!(q8.fits);
}

#[test]
fn legacy_profile_json_without_precision_deserializes_as_f16() {
    // A cached/exported pre-field payload must load truthfully: everything ever
    // produced before the field existed WAS computed at f16.
    let legacy = r#"{"weights_bytes":1,"kv_cache_bytes":2,"total_bytes":3,"cap_bytes":4,"context_length":8192,"fits":true,"pressure":false}"#;
    let p: super::MemoryProfile = serde_json::from_str(legacy).unwrap();
    assert_eq!(p.kv_precision, KvPrecision::F16);
}
