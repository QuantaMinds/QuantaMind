use super::*;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::eval::readiness::types::{AgentPath, CliffStatus, Readiness, ReadinessVerdict};
use crate::persistence::prompts::schema::InferenceParams;
use crate::persistence::publish::canonical::canonical_hash;
use crate::persistence::publish::row::PublishContext;

fn verdict(model: &str, pass_k: Option<f64>, quant: Option<&str>) -> ModelVerdict {
    ModelVerdict {
        model: model.to_string(),
        backend: BackendKind::LlamaCpp,
        verdict: ReadinessVerdict {
            status: Readiness::Ready,
            blocking: vec!["secret task detail".into()],
            conditions: vec![],
            path: AgentPath::NativeFc,
            required_tier: Default::default(),
            cleared_tier: None,
        },
        memory: None,
        avg_steps: Some(3.0),
        effort: Some(1.2),
        pass_k,
        quantization: quant.map(|s| s.to_string()),
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

const COHORT: &str = "apple-silicon/m3-pro/32-64gb";

#[test]
fn projects_measured_rows_and_counts_excluded() {
    let verdicts = vec![
        verdict("qwen", Some(0.9), Some("Q4_K_M")),
        verdict("unmeasured", None, Some("Q4_K_M")),
        verdict("noquant", Some(0.8), None),
    ];
    let p = build_preview(&verdicts, &PublishContext::test_ctx(COHORT, "0.2.0")).unwrap();
    assert_eq!(p.rows.len(), 1);
    assert_eq!(p.excluded_count, 2);
    assert_eq!(p.rows[0].cohort_key, COHORT);
    assert!(p.invalid.is_none());
}

#[test]
fn projects_both_paths_of_a_dual_measured_model_as_two_rows() {
    // The Agent Report emits one verdict per measured path; build_preview projects each, so a
    // dual-measured model becomes TWO publish rows distinguished by `eval_method`. This is how
    // both native + prompt-based results reach the leaderboard from the Deploy button.
    let mut native = verdict("qwen", Some(0.82), Some("Q4_K_M"));
    native.verdict.path = AgentPath::NativeFc;
    let mut prompt = verdict("qwen", Some(0.74), Some("Q4_K_M"));
    prompt.verdict.path = AgentPath::PromptBased;
    let p = build_preview(&[native, prompt], &PublishContext::test_ctx(COHORT, "0.2.0")).unwrap();
    assert_eq!(p.rows.len(), 2);
    assert!(p.invalid.is_none()); // both survive pre-validation (no model dedup)
    let methods: Vec<AgentPath> = p.rows.iter().map(|r| r.eval_method).collect();
    assert!(methods.contains(&AgentPath::NativeFc) && methods.contains(&AgentPath::PromptBased));
    // Each row carries its OWN path's pass_k — not blended.
    let nat = p.rows.iter().find(|r| r.eval_method == AgentPath::NativeFc).unwrap();
    let pr = p.rows.iter().find(|r| r.eval_method == AgentPath::PromptBased).unwrap();
    assert_eq!(nat.metrics.pass_k, 0.82);
    assert_eq!(pr.metrics.pass_k, 0.74);
}

#[test]
fn hash_matches_the_canonical_hash_of_the_rows() {
    let verdicts = vec![verdict("qwen", Some(0.9), Some("Q4_K_M"))];
    let p = build_preview(&verdicts, &PublishContext::test_ctx(COHORT, "0.2.0")).unwrap();
    assert_eq!(p.hash, canonical_hash(&p.rows).unwrap());
}

#[test]
fn preview_payload_never_carries_task_content() {
    let verdicts = vec![verdict("qwen", Some(0.9), Some("Q4_K_M"))];
    let p = build_preview(&verdicts, &PublishContext::test_ctx(COHORT, "0.2.0")).unwrap();
    assert!(!p.canonical_json.contains("secret task detail"));
    assert!(!p.canonical_json.contains("\"blocking\""));
}

#[test]
fn preview_carries_the_run_params_each_row() {
    // The global-header params in effect are stamped onto every projected row, so the
    // leaderboard knows the sampling/context a pass_k was measured under.
    let params = InferenceParams { temperature: Some(0.2), num_ctx: Some(8192), ..Default::default() };
    let verdicts = vec![verdict("qwen", Some(0.9), Some("Q4_K_M"))];
    let mut ctx = PublishContext::test_ctx(COHORT, "0.2.0");
    ctx.params = params.clone();
    let p = build_preview(&verdicts, &ctx).unwrap();
    assert_eq!(p.rows[0].params, params);
    assert!(p.canonical_json.contains("temperature"));
    assert!(p.canonical_json.contains("num_ctx"));
    // Unset keys are omitted — never a fabricated default on the wire.
    assert!(!p.canonical_json.contains("top_p"));
}
