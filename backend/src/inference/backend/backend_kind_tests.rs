use super::*;

#[test]
fn llama_cpp_serializes_as_snake_case_string() {
    let json = serde_json::to_string(&BackendKind::LlamaCpp).unwrap();
    assert_eq!(json, "\"llama_cpp\"");
}

#[test]
fn llama_cpp_round_trips_through_serde() {
    let parsed: BackendKind = serde_json::from_str("\"llama_cpp\"").unwrap();
    assert_eq!(parsed, BackendKind::LlamaCpp);
}

#[test]
fn vllm_serializes_and_round_trips_as_vllm() {
    // Per-variant rename overrides rename_all: "vllm", not "v_llm".
    assert_eq!(serde_json::to_string(&BackendKind::VLlm).unwrap(), "\"vllm\"");
    let parsed: BackendKind = serde_json::from_str("\"vllm\"").unwrap();
    assert_eq!(parsed, BackendKind::VLlm);
}

#[test]
fn sglang_serializes_and_round_trips_as_sglang() {
    assert_eq!(serde_json::to_string(&BackendKind::SgLang).unwrap(), "\"sglang\"");
    let parsed: BackendKind = serde_json::from_str("\"sglang\"").unwrap();
    assert_eq!(parsed, BackendKind::SgLang);
}

#[test]
fn default_is_llama_cpp() {
    assert_eq!(BackendKind::default(), BackendKind::LlamaCpp);
}

#[test]
fn an_unknown_backend_string_is_rejected_rather_than_silently_defaulted() {
    // A report saved by another build (or a hand-edited one) naming a backend this
    // build doesn't have must fail loudly on load, not be silently reinterpreted as
    // llama.cpp — that would relabel someone else's measurements as this backend's
    // (metric comparability).
    for unknown in ["\"nope\"", "\"llama_cpp_v2\"", "\"\""] {
        assert!(
            serde_json::from_str::<BackendKind>(unknown).is_err(),
            "{unknown} must not deserialize"
        );
    }
}
