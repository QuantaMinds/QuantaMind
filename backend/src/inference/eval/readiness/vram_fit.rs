use crate::inference::vram_math::{kv_cache_bytes_at, KvPrecision};
use serde::{Deserialize, Serialize};

/// A model fits but sits at/above this fraction of the cap → flag VRAM pressure
/// (a soft Conditional, not a block). NOT the same concept as `fit.ts`'s
/// `TIGHT_FRACTION` (0.7, a pre-download heuristic with a 1.3× safety multiplier)
/// — do not unify them. The frontend mirrors THIS constant in
/// `frontend/src/shared/memory/pressure.ts`; a drift-guard test below pins the two
/// to the same value.
pub const PRESSURE_FRACTION: f64 = 0.85;

/// Fallback context for the KV-cache estimate when the run pinned no `num_ctx`.
/// Sizing at the model's *max* context (262 k on newer models) yields an absurd
/// 100+ GB cache that nobody actually allocates; an 8 k dev window is the realistic
/// default. An explicit run `num_ctx` always wins — this only caps the fallback.
pub const DEFAULT_FALLBACK_CTX: u32 = 8192;

/// One model's memory-footprint ESTIMATE against an allocation cap: the weights
/// term is exact (on-disk file size), the KV term is computed by the canonical
/// formula at the run's context length and stated cache precision — nothing here
/// is read back from a running process. `kv_precision` makes every profile
/// SELF-DESCRIBING: a fit graded at a
/// Q8 cache (what a llama.cpp launch would actually use under memory pressure) can
/// never be silently compared against an f16-graded one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryProfile {
    pub weights_bytes: u64,
    pub kv_cache_bytes: u64,
    pub total_bytes: u64,
    pub cap_bytes: u64,
    pub context_length: u32,
    pub fits: bool,
    pub pressure: bool,
    /// The KV cache was sized from a defaulted `head_count_kv` (the model didn't
    /// report one) → a conservative overestimate. The UI labels the fit "estimated".
    #[serde(default)]
    pub estimated: bool,
    /// KV-cache storage precision this fit was graded at. `#[serde(default)]` =
    /// F16 — truthful for every profile produced before the field existed.
    #[serde(default)]
    pub kv_precision: KvPrecision,
}

/// Pure VRAM-fit estimate: weights + KV cache (via the canonical `vram_math`
/// formula at `precision`) vs the cap. `fits` = total ≤ cap; `pressure` = fits
/// but ≥85% of the cap. Takes dimension primitives (not `commands`' `ModelDims`)
/// so `inference/` stays Tauri-free and the future CLI shares the same math.
#[allow(clippy::too_many_arguments)]
pub fn estimate(
    weights_bytes: u64,
    layers: u64,
    head_count: u64,
    head_count_kv: u64,
    embedding_length: u64,
    context_length: u32,
    cap_bytes: u64,
    precision: KvPrecision,
) -> MemoryProfile {
    let kv_cache_bytes =
        kv_cache_bytes_at(precision, layers, head_count, head_count_kv, embedding_length, context_length as u64);
    let total_bytes = weights_bytes.saturating_add(kv_cache_bytes);
    let fits = total_bytes <= cap_bytes;
    let pressure = fits && cap_bytes > 0 && total_bytes as f64 >= cap_bytes as f64 * PRESSURE_FRACTION;
    MemoryProfile {
        weights_bytes,
        kv_cache_bytes,
        total_bytes,
        cap_bytes,
        context_length,
        fits,
        pressure,
        estimated: false,
        kv_precision: precision,
    }
}

/// Transformer dimensions for the KV-cache estimate, mirrored from `commands`'
/// `ModelDims` so `inference/` doesn't depend on the IPC layer. `context_length`
/// is the model's max context (the fallback when a run had no `num_ctx`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dims {
    pub layers: u64,
    pub head_count: u64,
    pub head_count_kv: u64,
    pub embedding_length: u64,
    pub context_length: u32,
    /// `head_count_kv` was defaulted (model didn't report it) → the resulting fit
    /// is a conservative estimate; propagated to `MemoryProfile.estimated`.
    pub kv_estimated: bool,
}

/// Compute a memory profile only when every input is present: a cap, the exact
/// weight size, and real dims. Any `None` ⇒ `None` ("not measured" — the verdict
/// then treats VRAM as unmeasured, never a guessed fit). Sizes the cache to the
/// run's `num_ctx`, falling back to a realistic 8 k window (`DEFAULT_FALLBACK_CTX`)
/// capped by the model's max — never the model's full (e.g. 262 k) context, which
/// would balloon the cache far beyond any real workload.
pub fn try_profile(
    weights_bytes: Option<u64>,
    dims: Option<Dims>,
    num_ctx: Option<u32>,
    cap_bytes: Option<u64>,
    precision: KvPrecision,
) -> Option<MemoryProfile> {
    let (weights, d, cap) = (weights_bytes?, dims?, cap_bytes?);
    let ctx = num_ctx.unwrap_or_else(|| d.context_length.min(DEFAULT_FALLBACK_CTX));
    let mut profile = estimate(weights, d.layers, d.head_count, d.head_count_kv, d.embedding_length, ctx, cap, precision);
    profile.estimated = d.kv_estimated;
    Some(profile)
}

#[cfg(test)]
#[path = "vram_fit_tests.rs"]
mod tests;
