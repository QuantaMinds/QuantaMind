use serde::{Deserialize, Serialize};

/// f16 KV-cache size in bytes for a transformer at a given context length:
/// `2 (K+V) × layers × kv_heads × head_dim × 2 (bytes/f16) × ctx`, where
/// `head_dim = embedding_length / head_count`. The canonical, tested formula —
/// the frontend calls this (never re-implements it). Returns 0 if `head_count`
/// is 0 (avoids divide-by-zero on incomplete metadata).
pub fn calculate_kv_cache_bytes(
    layers: u64,
    head_count: u64,
    head_count_kv: u64,
    embedding_length: u64,
    context_length: u64,
) -> u64 {
    if head_count == 0 {
        return 0;
    }
    let head_dim = embedding_length / head_count;
    2 * layers * head_count_kv * head_dim * 2 * context_length
}

/// KV-cache storage precision. The wire names match the llama.cpp/Ollama cache
/// types (`f16`, `q8_0`, `q4_0`) — the three with clean element-size semantics
/// (2 / ~1 / ~0.5 bytes per value); other llama.cpp types (q5_x, iq4_nl) exist
/// but are not modeled. This is an element-size model: real quantized caches
/// add small per-block scale overhead, so figures derived from it are "≈".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum KvPrecision {
    #[default]
    #[serde(rename = "f16")]
    F16,
    #[serde(rename = "q8_0")]
    Q8,
    #[serde(rename = "q4_0")]
    Q4,
}

impl KvPrecision {
    /// Integer divisor vs the f16 baseline (2 B/value): f16=1, q8_0=2, q4_0=4.
    /// The f16 formula carries a literal ×2×2 factor, so /2 and /4 are exact —
    /// no floats anywhere in the byte math.
    pub fn divisor(self) -> u64 {
        match self {
            KvPrecision::F16 => 1,
            KvPrecision::Q8 => 2,
            KvPrecision::Q4 => 4,
        }
    }

    /// Parse a wire name; unknown/absent input falls back to F16 (the
    /// conservative baseline every pre-existing caller assumed).
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "f16" => Some(KvPrecision::F16),
            "q8_0" => Some(KvPrecision::Q8),
            "q4_0" => Some(KvPrecision::Q4),
            _ => None,
        }
    }
}

/// KV-cache bytes at a given storage precision — the f16 canonical formula
/// scaled by the precision's exact integer divisor.
pub fn kv_cache_bytes_at(
    precision: KvPrecision,
    layers: u64,
    head_count: u64,
    head_count_kv: u64,
    embedding_length: u64,
    context_length: u64,
) -> u64 {
    calculate_kv_cache_bytes(layers, head_count, head_count_kv, embedding_length, context_length)
        / precision.divisor()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llama3_8b_kv_at_8k_is_one_gib() {
        // Llama-3-8B (GQA): 32 layers, 32 heads, 8 KV heads, 4096 emb, head_dim 128.
        // 2 × 32 × 8 × 128 × 2 × 8192 = 1,073,741,824 bytes = exactly 1 GiB.
        let kv = calculate_kv_cache_bytes(32, 32, 8, 4096, 8192);
        assert_eq!(kv, 1_073_741_824);
    }

    #[test]
    fn scales_linearly_with_context() {
        let a = calculate_kv_cache_bytes(32, 32, 8, 4096, 8192);
        let b = calculate_kv_cache_bytes(32, 32, 8, 4096, 16384);
        assert_eq!(b, a * 2);
    }

    #[test]
    fn zero_head_count_does_not_panic() {
        assert_eq!(calculate_kv_cache_bytes(32, 0, 8, 4096, 8192), 0);
    }

    #[test]
    fn q8_kv_is_exactly_half_of_f16_and_q4_a_quarter() {
        // Llama-3-8B @ 8k: f16 = exactly 1 GiB → q8_0 = 512 MiB, q4_0 = 256 MiB.
        let f16 = kv_cache_bytes_at(KvPrecision::F16, 32, 32, 8, 4096, 8192);
        let q8 = kv_cache_bytes_at(KvPrecision::Q8, 32, 32, 8, 4096, 8192);
        let q4 = kv_cache_bytes_at(KvPrecision::Q4, 32, 32, 8, 4096, 8192);
        assert_eq!(f16, 1_073_741_824);
        assert_eq!(q8, 536_870_912);
        assert_eq!(q4, 268_435_456);
    }

    #[test]
    fn kv_at_f16_equals_legacy_formula() {
        for (l, h, kv, emb, ctx) in [(32u64, 32u64, 8u64, 4096u64, 8192u64), (48, 40, 8, 5120, 32768), (28, 16, 16, 2048, 4096)] {
            assert_eq!(
                kv_cache_bytes_at(KvPrecision::F16, l, h, kv, emb, ctx),
                calculate_kv_cache_bytes(l, h, kv, emb, ctx)
            );
        }
    }

    #[test]
    fn precision_parses_wire_names_and_rejects_unknown() {
        assert_eq!(KvPrecision::parse("f16"), Some(KvPrecision::F16));
        assert_eq!(KvPrecision::parse("q8_0"), Some(KvPrecision::Q8));
        assert_eq!(KvPrecision::parse("q4_0"), Some(KvPrecision::Q4));
        assert_eq!(KvPrecision::parse("q5_1"), None); // exists in llama.cpp, not modeled
        assert_eq!(KvPrecision::parse(""), None);
    }

    #[test]
    fn precision_serializes_to_wire_names() {
        assert_eq!(serde_json::to_string(&KvPrecision::Q8).unwrap(), "\"q8_0\"");
        assert_eq!(serde_json::from_str::<KvPrecision>("\"q4_0\"").unwrap(), KvPrecision::Q4);
    }
}
