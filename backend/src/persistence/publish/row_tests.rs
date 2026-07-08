use super::*;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::eval::agentic::scoring::report::FailureTracker;
use crate::inference::eval::batch::TierStat;
use crate::inference::eval::readiness::types::{CliffStatus, ReadinessVerdict};

fn verdict(model: &str, pass_k: Option<f64>, quant: Option<&str>) -> ModelVerdict {
    ModelVerdict {
        model: model.to_string(),
        backend: BackendKind::Ollama,
        verdict: ReadinessVerdict {
            status: Readiness::Conditional,
            blocking: vec!["this reason must never reach the wire".into()],
            conditions: vec![],
            path: AgentPath::NativeFc,
            required_tier: Tier::Hard,
            cleared_tier: Some(Tier::Medium),
        },
        memory: None,
        avg_steps: Some(3.0),
        effort: Some(1.2),
        pass_k,
        quantization: quant.map(|s| s.to_string()),
        cliff: CliffStatus::NotProbed,
        by_tier: vec![
            TierStat { tier: Tier::Easy, tasks_passed: 5, tasks_total: 5, avg_steps: Some(2.0), failures: FailureTracker::default() },
            TierStat { tier: Tier::Medium, tasks_passed: 6, tasks_total: 8, avg_steps: Some(4.0), failures: FailureTracker::default() },
        ],
        failures: FailureTracker { hallucinated_completions: 2, forbidden_calls: 1, reported_in_prose_calls: 3, ..Default::default() },
        passes: 0,
        total_runs: 0,
        is_thinking: false,
        cpu_offloaded: false,
        ctx_ceiling: None,
        think_preset: Default::default(),
    }
}

#[test]
fn drops_unmeasured_unquantized_or_custom_collection_rows() {
    let ctx = PublishContext::test_ctx("apple-silicon/m3-pro/32-64gb", "0.2.0");
    // No measured pass_k → excluded.
    assert!(PublishRow::project(&verdict("m", None, Some("Q4_K_M")), &ctx).is_none());
    // No real quantization → excluded.
    assert!(PublishRow::project(&verdict("m", Some(0.8), None), &ctx).is_none());
    // Custom collection (no collection_hash) → excluded even though measured + quantized.
    let mut custom = PublishContext::test_ctx("c", "0.2.0");
    custom.collection_hash = None;
    assert!(PublishRow::project(&verdict("m", Some(0.8), Some("Q4_K_M")), &custom).is_none());
}

/// The right-sizing summary is host-specific and must NEVER reach the wire. It
/// isn't reachable from `ModelVerdict` (it's a sibling in `ReadinessAssessment`),
/// so this pins that a published row's serialized form carries no right-sizing key.
#[test]
fn published_row_carries_no_right_sizing_data() {
    let mut ctx = PublishContext::test_ctx("apple-silicon/m3-pro/32-64gb", "0.2.0");
    ctx.collection_hash = Some("abc123".into());
    let r = PublishRow::project(&verdict("qwen", Some(0.75), Some("Q4_K_M")), &ctx).unwrap();
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.contains("right_siz"), "no right-sizing data may leak to the publish wire: {json}");
    assert!(!json.contains("reduction_pct"), "no percent-reduction data on the wire: {json}");
}

/// rule 7f: the publish payload is the only thing that leaves the machine, so PROVE no
/// machine identity (username / home path) can ride out via its free-text fields, even when
/// the source model name / collection name is itself an absolute local path.
#[test]
fn published_row_carries_no_machine_identity() {
    let mut ctx = PublishContext::test_ctx("apple-silicon/m3-pro/32-64gb", "0.2.0");
    ctx.collection_hash = Some("abc123".into());
    ctx.collection_name = "/Users/alice/private-collection".into();
    let r = PublishRow::project(
        &verdict("/Users/alice/models/secret.gguf", Some(0.8), Some("Q4_K_M")),
        &ctx,
    )
    .expect("measured built-in projects");
    let json = serde_json::to_string(&r).unwrap();
    // The username (the machine identity) must not survive anywhere in the wire form.
    assert!(!json.contains("alice"), "username leaked to the publish wire: {json}");
    // redact_path masked it to <user> rather than dropping the field.
    assert!(r.model.contains("<user>"), "model not redacted: {}", r.model);
    assert!(r.collection_name.contains("<user>"), "collection not redacted: {}", r.collection_name);
}

/// Allowlist guard: a serialized row must contain ONLY known fields, so a newly-added
/// `PublishRow` field can't silently start shipping something identifying. If this fails,
/// add the field to `ALLOWED` on purpose (and confirm it carries no PII/path/host).
#[test]
fn serialized_row_has_only_allowlisted_fields() {
    const ALLOWED: &[&str] = &[
        "model", "quant", "cohort_key", "tool_version", "metrics", "params", "status",
        "eval_method", "tier_tested", "cleared_tier", "hardware_class", "recommended_tier",
        "by_tier", "failure_distribution", "is_thinking", "think_preset", "think_budget",
        "ctx_ceiling", "cpu_offloaded", "collection_name", "collection_hash", "schema_version",
        "engine_version", "build_hash",
    ];
    let mut ctx = PublishContext::test_ctx("apple-silicon/m3-pro/32-64gb", "0.2.0");
    ctx.collection_hash = Some("abc123".into());
    let mut thinking = verdict("qwen", Some(0.8), Some("Q4_K_M"));
    thinking.is_thinking = true; // populate the optional think_budget so it's covered too
    let r = PublishRow::project(&thinking, &ctx).unwrap();
    let val = serde_json::to_value(&r).unwrap();
    for key in val.as_object().unwrap().keys() {
        assert!(ALLOWED.contains(&key.as_str()), "unexpected field on the publish wire: {key}");
    }
}

#[test]
fn projects_the_full_verdict_by_allowlist() {
    let mut ctx = PublishContext::test_ctx("apple-silicon/m3-pro/32-64gb", "0.2.0");
    ctx.decoys_by_tier.insert(Tier::Medium, 4);
    ctx.collection_name = "hard-coding".into();
    ctx.collection_hash = Some("abc123".into());
    let r = PublishRow::project(&verdict("qwen", Some(0.75), Some("Q4_K_M")), &ctx).expect("measured built-in projects");

    assert_eq!(r.metrics.pass_k, 0.75);
    assert_eq!(r.status, Readiness::Conditional);
    assert_eq!(r.eval_method, AgentPath::NativeFc);
    assert_eq!(r.cleared_tier, Some(Tier::Medium));
    assert_eq!(r.tier_tested, Some(Tier::Medium)); // highest tier present in by_tier
    assert_eq!(r.hardware_class, HardwareClass::Mainstream);
    assert_eq!(r.recommended_tier, Tier::Medium);
    assert_eq!(r.collection_name, "hard-coding");
    assert_eq!(r.collection_hash, "abc123");
    assert_eq!(r.schema_version, PUBLISH_SCHEMA_VERSION);
    assert_eq!(r.engine_version, "0.2.0");
    assert_eq!(r.build_hash, "testhash");

    // by_tier: rate = passed/total, k from the tier, decoy_count from ctx axes.
    assert_eq!(r.by_tier.len(), 2);
    let easy = &r.by_tier[0];
    assert_eq!(easy.tier, Tier::Easy);
    assert_eq!(easy.pass_k_rate, 1.0);
    assert_eq!(easy.k, 5);
    assert_eq!(easy.decoy_count, None); // no axis declared for Easy
    let medium = &r.by_tier[1];
    assert_eq!(medium.pass_k_rate, 6.0 / 8.0);
    assert_eq!(medium.k, 8);
    assert_eq!(medium.decoy_count, Some(4));

    // failure distribution mapped field-by-field from the tracker.
    assert_eq!(r.failure_distribution.hallucinated, 2);
    assert_eq!(r.failure_distribution.forbidden_calls, 1);
    assert_eq!(r.failure_distribution.reported_in_prose, 3);
    assert_eq!(r.failure_distribution.infinite_loop, 0);
}

#[test]
fn reasoning_budget_projects_the_real_scratchpad_cap_at_the_tested_tier() {
    use crate::inference::eval::agentic::difficulty::passk::{think_tokens_for_preset, ThinkPreset};
    let ctx = PublishContext::test_ctx("apple-silicon/m3-pro/32-64gb", "0.2.0");

    // A reasoning model at Deep: think_budget is the REAL scratchpad cap for the tested tier
    // (Medium, the highest in by_tier), never fabricated. is_thinking/ctx_ceiling/preset carry through.
    let mut thinking = verdict("qwen-thinking", Some(0.8), Some("Q4_K_M"));
    thinking.is_thinking = true;
    thinking.think_preset = ThinkPreset::Deep;
    thinking.ctx_ceiling = Some(32768);
    thinking.cpu_offloaded = true;
    let r = PublishRow::project(&thinking, &ctx).expect("measured built-in projects");
    assert!(r.is_thinking);
    assert_eq!(r.think_preset, ThinkPreset::Deep);
    assert_eq!(r.think_budget, Some(think_tokens_for_preset(Tier::Medium, ThinkPreset::Deep)));
    assert_eq!(r.ctx_ceiling, Some(32768));
    assert!(r.cpu_offloaded);

    // A terse model NEVER gets a fabricated budget — think_budget is None.
    let terse = verdict("llama-terse", Some(0.8), Some("Q4_K_M"));
    let r = PublishRow::project(&terse, &ctx).expect("measured built-in projects");
    assert!(!r.is_thinking);
    assert_eq!(r.think_budget, None);
}
