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
