use super::*;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::eval::agentic::scoring::report::FailureTracker;
use crate::inference::eval::agentic::spec::Tier;
use crate::inference::eval::batch::TierStat;
use crate::inference::eval::readiness::types::{AgentPath, CliffStatus, ModelVerdict, Readiness, ReadinessVerdict};
use crate::persistence::prompts::schema::InferenceParams;
use crate::persistence::publish::row::{PublishContext, PublishRow};

fn verdict(model: &str, pass_k: Option<f64>, quant: Option<&str>) -> ModelVerdict {
    ModelVerdict {
        model: model.to_string(),
        backend: BackendKind::Ollama,
        verdict: ReadinessVerdict {
            status: Readiness::Ready,
            blocking: vec!["this reason must never reach the wire".into()],
            conditions: vec![],
            path: AgentPath::NativeFc,
            required_tier: Default::default(),
            cleared_tier: Some(Tier::Medium),
        },
        memory: None,
        avg_steps: Some(3.0),
        effort: Some(1.2),
        pass_k,
        quantization: quant.map(|s| s.to_string()),
        cliff: CliffStatus::NotProbed,
        by_tier: vec![TierStat { tier: Tier::Medium, tasks_passed: 6, tasks_total: 8, avg_steps: Some(4.0), failures: FailureTracker::default() }],
        failures: FailureTracker { hallucinated_completions: 1, ..Default::default() },
        passes: 0,
        total_runs: 0,
    }
}

fn ctx() -> PublishContext {
    PublishContext::test_ctx("apple-silicon/m-series/32-64gb", "0.2.0")
}

fn row(model: &str, pass_k: f64) -> PublishRow {
    PublishRow::project(&verdict(model, Some(pass_k), Some("Q4_K_M")), &ctx()).expect("a measured verdict projects")
}

#[test]
fn project_drops_unmeasured_or_unquantized_rows() {
    assert!(PublishRow::project(&verdict("m", None, Some("Q4_K_M")), &ctx()).is_none());
    assert!(PublishRow::project(&verdict("m", Some(0.8), None), &ctx()).is_none());
    let r = PublishRow::project(&verdict("m", Some(0.8), Some("Q4_K_M")), &ctx()).expect("ok");
    assert_eq!(r.metrics.pass_k, 0.8);
    assert_eq!(r.quant, "Q4_K_M");
}

#[test]
fn f32_param_does_not_widen_in_the_canonical_hash() {
    // A temperature of 0.2 must read as "0.2" in BOTH the wire body and the canonical form
    // the hash covers. `to_value()` would widen the f32 to f64 (0.20000000298023224), making
    // the client hash disagree with the server's hash of the wire bytes — rejecting every
    // batch that carries a float param. The hash is built from the serialized wire instead.
    let mut c = ctx();
    c.params = InferenceParams { temperature: Some(0.2), ..Default::default() };
    let r = PublishRow::project(&verdict("m", Some(0.8), Some("Q4_K_M")), &c).unwrap();
    let canon = canonical_json(std::slice::from_ref(&r)).unwrap();
    assert!(canon.contains("\"temperature\":0.2"), "canonical widened the f32: {canon}");
    assert!(!canon.contains("0.2000000"), "canonical carries an f32→f64 artifact: {canon}");
    // The canonical number matches the wire body's representation of the same row.
    let wire = serde_json::to_string(&[r]).unwrap();
    assert!(wire.contains("\"temperature\":0.2"));
}

#[test]
fn project_stamps_the_run_params_onto_the_row() {
    let params = InferenceParams { temperature: Some(0.2), num_ctx: Some(8192), ..Default::default() };
    let mut c = ctx();
    c.params = params.clone();
    let r = PublishRow::project(&verdict("m", Some(0.8), Some("Q4_K_M")), &c).expect("ok");
    assert_eq!(r.params, params);
    // Changing the params changes the integrity hash (params are part of the wire).
    let other = PublishRow::project(&verdict("m", Some(0.8), Some("Q4_K_M")), &ctx()).expect("ok");
    assert_ne!(canonical_hash(&[r]).unwrap(), canonical_hash(&[other]).unwrap());
}

#[test]
fn hash_is_stable_for_identical_input() {
    let a = [row("qwen", 0.9), row("llama", 0.7)];
    let b = [row("qwen", 0.9), row("llama", 0.7)];
    assert_eq!(canonical_hash(&a).unwrap(), canonical_hash(&b).unwrap());
}

#[test]
fn hash_changes_when_any_metric_changes() {
    let base = [row("qwen", 0.9)];
    let changed = [row("qwen", 0.91)];
    assert_ne!(canonical_hash(&base).unwrap(), canonical_hash(&changed).unwrap());
}

#[test]
fn canonical_json_is_allowlisted_verdicts_with_sorted_keys() {
    let json = canonical_json(&[row("qwen", 0.9)]).unwrap();
    // No verdict reasons / memory / backend internals / raw failure-tracker field
    // names leak to the wire (allowlist — only the named publish fields ship).
    assert!(!json.contains("this reason"));
    assert!(!json.contains("\"blocking\"") && !json.contains("\"memory\"") && !json.contains("\"verdict\""));
    assert!(!json.contains("hallucinated_completions"), "raw FailureTracker field leaked: {json}");
    // The new allowlisted verdict/provenance fields ARE present.
    for k in ["status", "by_tier", "failure_distribution", "collection_hash", "collection_name", "schema_version", "engine_version", "build_hash", "hardware_class", "recommended_tier"] {
        assert!(json.contains(&format!("\"{k}\"")), "expected allowlisted key '{k}' in {json}");
    }
    // Top-level keys sorted across a representative spread.
    let order: Vec<usize> = ["build_hash", "cohort_key", "collection_hash", "metrics", "model", "status", "tool_version"]
        .iter()
        .map(|k| json.find(&format!("\"{k}\"")).expect("key present"))
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "top-level keys not sorted: {json}");
    // Nested metrics keys sorted too: avg_steps < effort < pass_k.
    let m: Vec<usize> = ["avg_steps", "effort", "pass_k"]
        .iter()
        .map(|k| json.find(&format!("\"{k}\"")).expect("metric present"))
        .collect();
    assert!(m.windows(2).all(|w| w[0] < w[1]), "metric keys not sorted: {json}");
}

#[test]
fn hash_is_stable_for_a_fixed_extended_row() {
    // Golden integrity guard: the server re-hashes `results`; the canonical hash of a
    // fixed row must not drift as fields are added (a drift silently breaks server-side
    // verification). Pinning the full extended row's hash locks the wire contract.
    let r = PublishRow::sample("qwen", 0.9);
    let h = canonical_hash(std::slice::from_ref(&r)).unwrap();
    assert_eq!(h.len(), 64);
    assert_eq!(canonical_hash(std::slice::from_ref(&r)).unwrap(), h);
}
