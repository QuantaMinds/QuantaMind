use super::*;

/// One test: the in-memory fallback is a shared process-global, so splitting across
/// parallel tests would race. Uses a dedicated test key so it never disturbs real
/// entries, and clears it on both stores at the end.
#[test]
fn store_get_clear_round_trip_via_session_fallback() {
    const K: &str = "unit-test-secret";
    clear(K);
    assert_eq!(get(K), None);

    // Degrade path: no keychain entry, value kept in mem → get returns the session copy.
    mem().lock_recover().insert(K.to_string(), "sess".to_string());
    assert_eq!(get(K).as_deref(), Some("sess"));

    // clear forgets it on both stores — never panics.
    clear(K);
    assert_eq!(get(K), None);

    // Write-through: store ALWAYS keeps a session copy, readable even if the keychain
    // is later denied (Keychain or SessionOnly — either way mem holds it).
    let outcome = store(K, "written");
    assert!(matches!(outcome, Persisted::Keychain | Persisted::SessionOnly));
    assert_eq!(get(K).as_deref(), Some("written"));

    // Memory-first precedence: a populated session copy wins without touching keychain.
    mem().lock_recover().insert(K.to_string(), "mem-wins".to_string());
    assert_eq!(get(K).as_deref(), Some("mem-wins"));

    clear(K);
    assert_eq!(get(K), None);
}

/// Distinct keys don't collide (proves the keyed map, vs the single-entry auth vault).
#[test]
fn distinct_keys_are_independent() {
    const A: &str = "unit-test-a";
    const B: &str = "unit-test-b";
    mem().lock_recover().insert(A.to_string(), "aaa".to_string());
    mem().lock_recover().insert(B.to_string(), "bbb".to_string());
    assert_eq!(get(A).as_deref(), Some("aaa"));
    assert_eq!(get(B).as_deref(), Some("bbb"));
    clear(A);
    assert_eq!(get(A), None);
    assert_eq!(get(B).as_deref(), Some("bbb"));
    clear(B);
}
