//! Payload noise: wrap a getter's real blob in a realistic messy envelope so the model has
//! to extract the right field from noise (extra metadata, timestamps, pagination, nesting) —
//! the way real tool APIs return data. Every field is a pure function of the seed, so the
//! same call yields byte-identical bytes (the temp-0 reproducibility contract; never a wall
//! clock or RNG).

/// FNV-1a over a canonical call string → a stable per-call seed.
pub fn seed_from(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Wrap `blob` (a getter's real JSON) under `data`, alongside deterministic synthetic
/// metadata seeded from `seed`. The real answer is still present — nested — so the model must
/// reach for `data.<field>` rather than a top-level field, past distractor numbers
/// (`latency_ms`, `pagination.total`) that are NOT the answer.
pub fn wrap(blob: &str, seed: u64) -> String {
    let req = seed % 1_000_000;
    let latency = 20 + seed % 380; // 20..=399 ms
    let total = 3 + seed % 47; // a plausible-but-wrong count distractor
    let cached = seed % 2 == 0;
    let day = 1 + seed % 28; // fixed synthetic date bucket — no wall clock
    format!(
        r#"{{"data":{blob},"_meta":{{"request_id":"req_{req:06}","timestamp":"2026-02-{day:02}T09:14:07Z","latency_ms":{latency},"cached":{cached},"api_version":"v2"}},"pagination":{{"page":1,"per_page":50,"total":{total}}},"warnings":[]}}"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn wrap_nests_the_blob_under_data_and_is_valid_json() {
        let out = wrap(r#"{"balance":450}"#, seed_from("read_account|{\"id\":\"A-1\"}"));
        let v: Value = serde_json::from_str(&out).expect("noisy envelope is valid JSON");
        // The real answer is still reachable, nested under `data`.
        assert_eq!(v["data"]["balance"], 450);
        // ...surrounded by realistic distractor metadata.
        assert!(v["_meta"]["request_id"].is_string());
        assert!(v["pagination"]["total"].is_number());
    }

    #[test]
    fn wrap_is_deterministic_for_the_same_seed() {
        let s = seed_from("read_account|{\"id\":\"A-1\"}");
        assert_eq!(wrap(r#"{"balance":450}"#, s), wrap(r#"{"balance":450}"#, s)); // reproducibility
    }

    #[test]
    fn the_answer_value_survives_verbatim() {
        // Grounding relies on the target value remaining a substring of the response.
        let out = wrap(r#"{"balance":"450.00"}"#, 12345);
        assert!(out.contains("450.00"));
    }
}
