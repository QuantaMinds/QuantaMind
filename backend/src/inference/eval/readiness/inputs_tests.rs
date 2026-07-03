use super::super::profile::{builtins, ReadinessProfile};
use super::super::types::{AgentPath, CliffStatus, NativeFcStatus, Readiness};
use super::{
    agentic_metrics, assess_report, from_column, merge_by_tier, merged_by_tier_for, merged_by_tier_for_path,
    pass_k_of, resolve_quant, verdict_for, verdicts_for_column,
};
use crate::inference::eval::batch::BatchReport;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::eval::agentic::scoring::report::{FailureTracker, TopError};
use crate::inference::eval::batch::{AggAgentic, BatchColumn};

fn col(passes: u32, total: u32, loops: u32, hall: u32, steps: Option<f64>) -> BatchColumn {
    BatchColumn {
        model: "m".into(),
        backend: BackendKind::Ollama,
        toolcall: None,
        agentic: Some(AggAgentic {
            tasks_passed: passes,
            tasks_total: total,
            passes,
            total_runs: total,
            avg_steps: steps,
            avg_output_tokens_success: None,
            schema_resilience: None,
            top_error: TopError::None,
            failures: FailureTracker {
                infinite_loop_hits: loops,
                hallucinated_completions: hall,
                malformed_json_calls: 0,
                schema_unrecovered_calls: 0,
                unknown_tool_calls: 0,
                forbidden_calls: 0,
                turn_timeouts: 0,
                reported_in_prose_calls: 0,
                foreign_dialect_calls: 0,
                empty_output_calls: 0,
                truncated_calls: 0,
                reasoning_overrun_calls: 0,
            },
            by_tier: vec![],
            tasks_errored: 0,
            native_error_class: Default::default(),
        }),
        agentic_native_fc: None,
        error: None,
        is_thinking: false,
            cpu_offloaded: false,
            ctx_ceiling: None,
    }
}

#[test]
fn agentic_column_maps_pass_k_steps_and_failure_counts() {
    let i = from_column(&col(4, 5, 1, 2, Some(3.0)), None, false, CliffStatus::NotProbed);
    assert_eq!(i.pass_k, Some(0.8));
    assert_eq!(i.avg_steps, Some(3.0));
    assert_eq!(i.loops, 1);
    assert_eq!(i.hallucinated, 2);
}

#[test]
fn deferred_metrics_are_not_measured_never_fabricated() {
    let i = from_column(&col(5, 5, 0, 0, Some(2.0)), None, false, CliffStatus::NotProbed);
    assert_eq!(i.ms_per_step, None);
    assert_eq!(i.cliff, CliffStatus::NotProbed);
    assert_eq!(i.fits_in_vram, None);
    assert_eq!(i.native_fc, NativeFcStatus::NotSupported);
}

#[test]
fn no_agentic_column_yields_unmeasured_pass_k_core_gate() {
    let c = BatchColumn {
        model: "m".into(),
        backend: BackendKind::Ollama,
        toolcall: None,
        agentic: None,
        agentic_native_fc: None,
        error: None,
        is_thinking: false,
            cpu_offloaded: false,
            ctx_ceiling: None,
    };
    let i = from_column(&c, None, false, CliffStatus::NotProbed);
    assert_eq!(i.pass_k, None);
    assert_eq!(i.loops, 0);
}

#[test]
fn zero_total_runs_is_unmeasured_not_a_fabricated_zero() {
    let i = from_column(&col(0, 0, 0, 0, None), None, false, CliffStatus::NotProbed);
    assert_eq!(i.pass_k, None); // None, not Some(0.0) — never a guessed failing score
}

fn ctx_profile(min_ctx: Option<u32>) -> ReadinessProfile {
    ReadinessProfile {
        id: "t".into(),
        name: "T".into(),
        min_pass_k: 0.5,
        max_avg_steps: None,
        max_ms_per_step: None,
        min_context_tokens: min_ctx,
        forbid_infinite_loop: false,
        forbid_hallucinated_completion: false,
        require_full_vram: false,
        require_native_fc: false,
        required_tier: crate::inference::eval::agentic::spec::Tier::Easy,
    }
}

#[test]
fn context_cliff_gate_blocks_a_measured_shortfall_but_only_cautions_when_unmeasured() {
    let c = col(9, 10, 0, 0, Some(2.0)); // pass^k 0.9 clears the 0.5 bar

    // Gate OFF (None): the cliff is carried but never blocks → Ready.
    assert_eq!(verdict_for(&c, None, false, CliffStatus::Collapsed { depth: 8000 }, &ctx_profile(None)).status, Readiness::Ready);

    // Gate ON, cliff BELOW the floor (MEASURED failure) → NotReady with the reason.
    let below = verdict_for(&c, None, false, CliffStatus::Collapsed { depth: 8000 }, &ctx_profile(Some(16000)));
    assert_eq!(below.status, Readiness::NotReady);
    assert!(below.blocking.iter().any(|b| b.contains("8000") && b.contains("16000")), "{:?}", below.blocking);

    // Gate ON, cliff UNMEASURED → Conditional caveat (never a guessed pass, never a red block).
    let unmeasured = verdict_for(&c, None, false, CliffStatus::NotProbed, &ctx_profile(Some(16000)));
    assert_eq!(unmeasured.status, Readiness::Conditional);
    assert!(unmeasured.blocking.is_empty(), "unmeasured must not block: {:?}", unmeasured.blocking);
    assert!(unmeasured.conditions.iter().any(|c| c.to_lowercase().contains("not measured")), "{:?}", unmeasured.conditions);

    // Gate ON, cliff ABOVE the floor → passes (Ready).
    assert_eq!(verdict_for(&c, None, false, CliffStatus::Collapsed { depth: 32000 }, &ctx_profile(Some(16000))).status, Readiness::Ready);
}

fn agg(passes: u32, total: u32, loops: u32) -> AggAgentic {
    AggAgentic {
        tasks_passed: passes,
        tasks_total: total,
        passes,
        total_runs: total,
        avg_steps: Some(2.0),
        avg_output_tokens_success: None,
        schema_resilience: None,
        top_error: TopError::None,
        failures: FailureTracker {
            infinite_loop_hits: loops,
            hallucinated_completions: 0,
            malformed_json_calls: 0,
            schema_unrecovered_calls: 0,
            unknown_tool_calls: 0,
            forbidden_calls: 0,
            turn_timeouts: 0,
            reported_in_prose_calls: 0,
            foreign_dialect_calls: 0,
            empty_output_calls: 0,
            truncated_calls: 0,
            reasoning_overrun_calls: 0,
        },
        by_tier: vec![],
        tasks_errored: 0,
        native_error_class: Default::default(),
    }
}

#[test]
fn prefers_the_native_aggregate_when_native_fc_was_measured() {
    let mut c = col(9, 10, 0, 0, Some(2.0)); // prompt-based pass^k 0.9
    c.agentic_native_fc = Some(agg(2, 10, 1)); // native pass^k 0.2, with a loop
    let i = from_column(&c, None, false, CliffStatus::NotProbed);
    assert_eq!(i.pass_k, Some(0.2)); // the native result, NOT the 0.9 prompt proxy
    assert_eq!(i.loops, 1); // native failure breakdown
    assert_eq!(i.native_fc, NativeFcStatus::Tested { pass_k: 0.2 });
}

#[test]
fn falls_back_to_prompt_based_when_native_was_not_measured() {
    let i = from_column(&col(8, 10, 0, 0, Some(2.0)), None, false, CliffStatus::NotProbed);
    assert_eq!(i.pass_k, Some(0.8));
    assert_eq!(i.native_fc, NativeFcStatus::NotSupported);
}

#[test]
fn a_prompt_passing_model_that_fails_native_is_not_ready_on_the_native_path() {
    let coding = builtins().into_iter().find(|p| p.id == "coding-agent").unwrap();
    let mut c = col(9, 10, 0, 0, Some(2.0)); // prompt 0.9 would pass coding-agent's 0.80
    c.agentic_native_fc = Some(agg(2, 10, 0)); // native 0.2 fails it
    let v = verdict_for(&c, Some(true), false, CliffStatus::NotProbed, &coding); // fits VRAM (coding requires it)
    assert_eq!(v.status, Readiness::NotReady);
    assert_eq!(v.path, AgentPath::NativeFc); // the report states the native path
    assert!(v.blocking.iter().any(|b| b.contains("pass^k 0.20 < 0.80 required")));
}

#[test]
fn ranking_puts_a_ready_model_first_regardless_of_column_order() {
    use super::super::recommend;
    let general = builtins().into_iter().find(|p| p.id == "general-agent").unwrap();
    let report = BatchReport {
        collection_id: "c".into(),
        num_ctx: None, ollama_version: None, collection_hash: None,
        think_preset: None,
        columns: vec![
            // NotReady first (no agentic → the core pass^k gate blocks)…
            BatchColumn {
                model: "bad".into(),
                backend: BackendKind::Ollama,
                toolcall: None,
                agentic: None,
                agentic_native_fc: None,
                error: None,
                is_thinking: false,
            cpu_offloaded: false,
            ctx_ceiling: None,
            },
            col(5, 5, 0, 0, Some(2.0)), // …a clean Ready model ("m") second.
        ],
    };
    let mut verdicts = assess_report(&report, &general);
    recommend::rank(&mut verdicts);
    assert_eq!(verdicts[0].model, "m"); // the Ready model is ranked first
    assert_eq!(verdicts[0].verdict.status, Readiness::Ready);
    assert_eq!(verdicts[1].model, "bad");
}

#[test]
fn pass_k_of_is_native_first_then_prompt_then_none() {
    // Native present → native pass^k (3/5 = 0.6), even with a different prompt rate.
    let mut c = col(5, 5, 0, 0, Some(2.0)); // prompt 5/5
    c.agentic_native_fc = Some(AggAgentic {
        tasks_passed: 3,
        tasks_total: 5,
        passes: 3,
        total_runs: 5,
        avg_steps: Some(4.0),
        avg_output_tokens_success: Some(120.0),
        schema_resilience: None,
        top_error: TopError::None,
        failures: FailureTracker::default(),
        by_tier: vec![],
        tasks_errored: 0,
        native_error_class: Default::default(),
    });
    assert_eq!(pass_k_of(&c), Some(0.6)); // native, not the prompt 1.0

    // No native → prompt pass^k (4/10 = 0.4).
    assert_eq!(pass_k_of(&col(4, 10, 0, 0, Some(2.0))), Some(0.4));

    // No agentic data → None (renders N/A, never fabricated).
    let bare = BatchColumn { model: "m".into(), backend: BackendKind::Ollama, toolcall: None, agentic: None, agentic_native_fc: None, error: None, is_thinking: false, cpu_offloaded: false, ctx_ceiling: None };
    assert_eq!(pass_k_of(&bare), None);
}

#[test]
fn agentic_metrics_prefers_native_then_falls_back_to_prompt() {
    // Prompt aggregate: avg_steps 2.0. Native aggregate: avg_steps 4.0, effort 120.
    let mut c = col(5, 5, 0, 0, Some(2.0));
    c.agentic_native_fc = Some(AggAgentic {
        tasks_passed: 3,
        tasks_total: 5,
        passes: 3,
        total_runs: 5,
        avg_steps: Some(4.0),
        avg_output_tokens_success: Some(120.0),
        schema_resilience: None,
        top_error: TopError::None,
        failures: FailureTracker::default(),
        by_tier: vec![],
        tasks_errored: 0,
        native_error_class: Default::default(),
    });
    let (steps, effort) = agentic_metrics(&c);
    assert_eq!(steps, Some(4.0)); // native, NOT the prompt 2.0 — same telemetry the verdict gated on
    assert_eq!(effort, Some(120.0));

    // No native measurement → fall back to the prompt aggregate.
    let (steps2, _) = agentic_metrics(&col(5, 5, 0, 0, Some(2.0)));
    assert_eq!(steps2, Some(2.0));
}

#[test]
fn assess_report_grades_clean_models_and_short_circuits_errors() {
    let general = builtins().into_iter().find(|p| p.id == "general-agent").unwrap();
    let report = BatchReport {
        collection_id: "c".into(),
        num_ctx: None, ollama_version: None, collection_hash: None,
        think_preset: None,
        columns: vec![
            col(5, 5, 0, 0, Some(2.0)), // clean → Ready
            BatchColumn {
                model: "boom".into(),
                backend: BackendKind::Ollama,
                toolcall: None,
                agentic: None,
                agentic_native_fc: None,
                error: Some("backend offline".into()),
                is_thinking: false,
            cpu_offloaded: false,
            ctx_ceiling: None,
            },
        ],
    };
    let verdicts = assess_report(&report, &general);
    assert_eq!(verdicts[0].model, "m");
    assert_eq!(verdicts[0].verdict.status, Readiness::Ready);
    assert_eq!(verdicts[1].verdict.status, Readiness::NotReady);
    assert!(verdicts[1].verdict.blocking[0].contains("backend offline")); // real error, not a synthesized score
}

#[test]
fn model_verdict_carries_by_tier_and_failures_from_the_native_first_source() {
    use crate::inference::eval::agentic::spec::Tier;
    use crate::inference::eval::batch::TierStat;
    let general = builtins().into_iter().find(|p| p.id == "general-agent").unwrap();

    // Prompt aggregate has an Easy tier; the NATIVE aggregate has a Hard tier + 3 forbidden
    // calls. With native measured, the verdict's per-tier breakdown + failures must come
    // from native — the exact source the gate read — not the prompt proxy.
    let mut c = col(9, 10, 0, 0, Some(2.0));
    c.agentic.as_mut().unwrap().by_tier =
        vec![TierStat { tier: Tier::Easy, tasks_passed: 1, tasks_total: 1, avg_steps: Some(2.0), failures: FailureTracker::default() }];
    let mut native = agg(9, 10, 0);
    native.by_tier = vec![TierStat {
        tier: Tier::Hard,
        tasks_passed: 2,
        tasks_total: 4,
        avg_steps: Some(7.0),
        failures: FailureTracker { forbidden_calls: 3, ..Default::default() },
    }];
    native.failures = FailureTracker { forbidden_calls: 3, ..Default::default() };
    c.agentic_native_fc = Some(native);

    let report = BatchReport { collection_id: "c".into(), num_ctx: None, ollama_version: None, collection_hash: None, think_preset: None, columns: vec![c] };
    let v = &assess_report(&report, &general)[0];
    assert_eq!(v.by_tier.len(), 1);
    assert_eq!(v.by_tier[0].tier, Tier::Hard); // native, NOT the prompt's Easy
    assert_eq!(v.by_tier[0].avg_steps, Some(7.0));
    assert_eq!(v.failures.forbidden_calls, 3);
}

#[test]
fn verdicts_for_column_emits_a_row_per_measured_path() {
    let general = builtins().into_iter().find(|p| p.id == "general-agent").unwrap();
    let mut c = col(9, 10, 0, 0, Some(2.0)); // prompt pass^k 0.9
    c.agentic_native_fc = Some(agg(7, 10, 0)); // native pass^k 0.7
    let rows = verdicts_for_column(&c, None, false, CliffStatus::NotProbed, None, None, &general, &[], Default::default());
    assert_eq!(rows.len(), 2); // one row per measured path
    // Native first (display order); each row's pass_k is sourced STRICTLY from its own pass.
    assert_eq!(rows[0].verdict.path, AgentPath::NativeFc);
    assert_eq!(rows[0].pass_k, Some(0.7));
    assert_eq!(rows[1].verdict.path, AgentPath::PromptBased);
    assert_eq!(rows[1].pass_k, Some(0.9));
}

#[test]
fn verdicts_for_column_prompt_only_yields_one_row() {
    let general = builtins().into_iter().find(|p| p.id == "general-agent").unwrap();
    let rows =
        verdicts_for_column(&col(9, 10, 0, 0, Some(2.0)), None, false, CliffStatus::NotProbed, None, None, &general, &[], Default::default());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].verdict.path, AgentPath::PromptBased);
}

#[test]
fn verdicts_for_column_errored_column_is_one_not_ready_row() {
    let general = builtins().into_iter().find(|p| p.id == "general-agent").unwrap();
    let c = BatchColumn {
        model: "boom".into(),
        backend: BackendKind::Ollama,
        toolcall: None,
        agentic: None,
        agentic_native_fc: None,
        error: Some("backend offline".into()),
        is_thinking: false,
            cpu_offloaded: false,
            ctx_ceiling: None,
    };
    let rows = verdicts_for_column(&c, None, false, CliffStatus::NotProbed, None, None, &general, &[], Default::default());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].verdict.status, Readiness::NotReady);
    assert!(rows[0].verdict.blocking[0].contains("backend offline"));
}

#[test]
fn require_native_fc_does_not_block_the_prompt_row_of_a_native_capable_model() {
    // require_native_fc forced on. The model ran BOTH passes at high pass^k. The prompt row
    // must be Ready — native is available at the MODEL level — and never blocked for "native
    // not supported". This is the honesty fix: path is decoupled from the native_fc gate.
    let mut p = builtins().into_iter().find(|p| p.id == "general-agent").unwrap();
    p.require_native_fc = true;
    let mut c = col(9, 10, 0, 0, Some(2.0)); // prompt 0.9
    c.agentic_native_fc = Some(agg(9, 10, 0)); // native 0.9 → model-level Tested
    let rows = verdicts_for_column(&c, None, false, CliffStatus::NotProbed, None, None, &p, &[], Default::default());
    let prompt = rows.iter().find(|r| r.verdict.path == AgentPath::PromptBased).unwrap();
    assert_eq!(prompt.verdict.status, Readiness::Ready, "{:?}", prompt.verdict.blocking);
    assert!(!prompt.verdict.blocking.iter().any(|b| b.contains("native")));
}

#[test]
fn merged_by_tier_for_path_never_crosses_paths() {
    use crate::inference::eval::agentic::spec::Tier;
    // A NATIVE ladder whose primary holds Easy. A tier sibling ran ONLY the prompt path
    // (no native pass). Merging the NATIVE ladder must NOT pull the sibling's prompt Hard
    // tier in — an honest gap, never the other path's data.
    let primary = vec![ts(Tier::Easy, 1, 1)];
    let prompt_only_sibling = report_with("qwen", BackendKind::Ollama, vec![ts(Tier::Hard, 1, 1)]);
    let native = merged_by_tier_for_path("qwen", BackendKind::Ollama, AgentPath::NativeFc, &primary, &[&prompt_only_sibling]);
    assert_eq!(native.len(), 1);
    assert_eq!(native[0].tier, Tier::Easy); // sibling's prompt Hard is NOT crossed in

    // Sanity: the SAME merge on the PROMPT path DOES pick up the sibling's Hard tier.
    let prompt =
        merged_by_tier_for_path("qwen", BackendKind::Ollama, AgentPath::PromptBased, &primary, &[&prompt_only_sibling]);
    assert_eq!(prompt.len(), 2);
}

fn ts(tier: crate::inference::eval::agentic::spec::Tier, passed: u32, total: u32) -> crate::inference::eval::batch::TierStat {
    crate::inference::eval::batch::TierStat { tier, tasks_passed: passed, tasks_total: total, avg_steps: None, failures: FailureTracker::default() }
}

/// A persisted report holding one model column whose native-first source carries `tiers`.
fn report_with(model: &str, backend: BackendKind, tiers: Vec<crate::inference::eval::batch::TierStat>) -> BatchReport {
    let mut a = agg(1, 1, 0);
    a.by_tier = tiers;
    BatchReport {
        collection_id: "x".into(),
        num_ctx: None, ollama_version: None, collection_hash: None,
        think_preset: None,
        columns: vec![BatchColumn {
            model: model.into(),
            backend,
            toolcall: None,
            agentic: Some(a),
            agentic_native_fc: None,
            error: None,
            is_thinking: false,
            cpu_offloaded: false,
            ctx_ceiling: None,
        }],
    }
}

#[test]
fn resolve_quant_prefers_registry_then_falls_back_to_the_model_name() {
    // Registry knows it → use that verbatim.
    assert_eq!(resolve_quant(Some("Q5_K_M".into()), "qwen2.5:7b"), Some("Q5_K_M".to_string()));
    // Registry blank (llama.cpp / MLX / Ollama offline) but the name encodes the quant →
    // parsed, so the publish row isn't dropped.
    assert_eq!(resolve_quant(None, "qwen2.5-7b-instruct-Q4_K_M.gguf"), Some("Q4_K_M".to_string()));
    // Neither source knows it → None (never fabricated).
    assert_eq!(resolve_quant(None, "some-model-no-quant"), None);
}

#[test]
fn merge_by_tier_unions_distinct_tiers_first_wins_and_sorts() {
    use crate::inference::eval::agentic::spec::Tier;
    // Selected (easy) first; a medium sibling adds its tier. Out-of-order input sorts.
    let hard = vec![ts(Tier::Hard, 1, 4)];
    let easy = vec![ts(Tier::Easy, 4, 5)];
    let medium = vec![ts(Tier::Medium, 3, 5)];
    let merged = merge_by_tier(&[&easy, &medium, &hard]);
    assert_eq!(merged.iter().map(|t| t.tier).collect::<Vec<_>>(), vec![Tier::Easy, Tier::Medium, Tier::Hard]);

    // Collision: the FIRST list wins (the selected collection is authoritative).
    let easy_a = vec![ts(Tier::Easy, 4, 5)];
    let easy_b = vec![ts(Tier::Easy, 0, 5)];
    let merged = merge_by_tier(&[&easy_a, &easy_b]);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].tasks_passed, 4); // from the first list, not the second
}

#[test]
fn merged_by_tier_for_accumulates_the_same_model_across_siblings() {
    use crate::inference::eval::agentic::spec::Tier;
    let primary = vec![ts(Tier::Easy, 4, 5)]; // selected collection: easy
    let medium = report_with("qwen", BackendKind::Ollama, vec![ts(Tier::Medium, 3, 5)]);
    let out = merged_by_tier_for("qwen", BackendKind::Ollama, &primary, &[&medium]);
    assert_eq!(out.iter().map(|t| t.tier).collect::<Vec<_>>(), vec![Tier::Easy, Tier::Medium]);
}

#[test]
fn merged_by_tier_for_never_pulls_a_different_models_or_backends_tiers() {
    use crate::inference::eval::agentic::spec::Tier;
    // The (model, backend) match key prevents a cross-model/backend "Frankenstein ladder":
    // a sibling for a DIFFERENT model — or the same name on a DIFFERENT backend — is ignored.
    let primary = vec![ts(Tier::Easy, 4, 5)];
    let other_model = report_with("llama", BackendKind::Ollama, vec![ts(Tier::Medium, 3, 5)]);
    let other_backend = report_with("qwen", BackendKind::LlamaCpp, vec![ts(Tier::Hard, 1, 4)]);
    let out = merged_by_tier_for("qwen", BackendKind::Ollama, &primary, &[&other_model, &other_backend]);
    assert_eq!(out.iter().map(|t| t.tier).collect::<Vec<_>>(), vec![Tier::Easy]); // easy only
}

#[test]
fn model_verdict_by_tier_falls_back_to_prompt_when_native_absent() {
    use crate::inference::eval::agentic::spec::Tier;
    use crate::inference::eval::batch::TierStat;
    let general = builtins().into_iter().find(|p| p.id == "general-agent").unwrap();
    let mut c = col(5, 5, 0, 0, Some(2.0)); // native absent (the common case — FC defaults off)
    c.agentic.as_mut().unwrap().by_tier = vec![TierStat {
        tier: Tier::Medium,
        tasks_passed: 1,
        tasks_total: 1,
        avg_steps: Some(3.0),
        failures: FailureTracker { unknown_tool_calls: 4, ..Default::default() },
    }];
    c.agentic.as_mut().unwrap().failures = FailureTracker { unknown_tool_calls: 4, ..Default::default() };
    let report = BatchReport { collection_id: "c".into(), num_ctx: None, ollama_version: None, collection_hash: None, think_preset: None, columns: vec![c] };
    let v = &assess_report(&report, &general)[0];
    assert_eq!(v.by_tier.len(), 1);
    assert_eq!(v.by_tier[0].tier, Tier::Medium);
    assert_eq!(v.failures.unknown_tool_calls, 4); // prompt-based source, since native absent
}
