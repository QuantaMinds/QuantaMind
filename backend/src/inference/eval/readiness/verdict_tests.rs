use super::super::profile::{builtins, ReadinessProfile};
use super::super::types::{AgentPath, CliffStatus, NativeFcStatus, Readiness, ReadinessInputs};
use super::assess;
use crate::inference::eval::agentic::spec::Tier;

/// A lenient profile: only the Pass^k core gate is active, everything else off.
fn lenient() -> ReadinessProfile {
    ReadinessProfile {
        id: "t".into(),
        name: "Test".into(),
        min_pass_k: 0.80,
        max_avg_steps: None,
        max_ms_per_step: None,
        min_context_tokens: None,
        forbid_infinite_loop: false,
        forbid_hallucinated_completion: false,
        require_full_vram: false,
        require_native_fc: false,
        required_tier: crate::inference::eval::agentic::spec::Tier::Easy,
    }
}

/// A model that passes everything: high Pass^k, no loops/hallucinations.
fn clean_inputs() -> ReadinessInputs {
    ReadinessInputs {
        pass_k: Some(0.95),
        avg_steps: Some(2.0),
        ms_per_step: Some(800),
        cliff: CliffStatus::NoCliff { tested: 16_384, saturated: false },
        fits_in_vram: Some(true),
        vram_pressure: false,
        kv_downgraded: false,
        loops: 0,
        hallucinated: 0,
        native_fc: NativeFcStatus::NotSupported,
        path: AgentPath::PromptBased,
        tier_pass_k: vec![],
    }
}

#[test]
fn clean_row_against_lenient_profile_is_ready() {
    let v = assess(&clean_inputs(), &lenient());
    assert_eq!(v.status, Readiness::Ready);
    assert!(v.blocking.is_empty() && v.conditions.is_empty());
}

/// Gate-at-actual-KV transparency: a fit graded at the launch's Q8 cache is
/// Conditional with the explicit advisory — never a silent f16-lookalike.
#[test]
fn q8_assumed_fit_is_conditional_with_advisory() {
    let i = ReadinessInputs { kv_downgraded: true, ..clean_inputs() };
    let v = assess(&i, &lenient());
    assert_eq!(v.status, Readiness::Conditional);
    assert!(
        v.conditions.iter().any(|c| c.contains("Q8 KV cache")),
        "advisory names the cache precision: {:?}",
        v.conditions
    );
}

/// The advisory rides along even when the fit BLOCKS — the row stays
/// self-describing in every outcome.
#[test]
fn q8_assumed_and_not_fitting_still_blocks_and_carries_advisory() {
    let mut p = lenient();
    p.require_full_vram = true;
    let i = ReadinessInputs { kv_downgraded: true, fits_in_vram: Some(false), ..clean_inputs() };
    let v = assess(&i, &p);
    assert_eq!(v.status, Readiness::NotReady);
    assert!(v.blocking.iter().any(|b| b.contains("offload")), "the bad fit blocks: {:?}", v.blocking);
    assert!(v.conditions.iter().any(|c| c.contains("Q8 KV cache")), "advisory still present: {:?}", v.conditions);
}

/// f16-graded fits carry NO cache advisory (pinned explicitly, beyond the clean
/// test's empty-conditions assertion).
#[test]
fn f16_fit_has_no_kv_advisory() {
    let v = assess(&clean_inputs(), &lenient());
    assert!(!v.conditions.iter().any(|c| c.contains("KV cache")), "{:?}", v.conditions);
}

#[test]
fn low_pass_k_blocks_with_interpolated_math() {
    let mut i = clean_inputs();
    i.pass_k = Some(0.40);
    let v = assess(&i, &lenient());
    assert_eq!(v.status, Readiness::NotReady);
    assert_eq!(v.blocking, vec!["pass^k 0.40 < 0.80 required".to_string()]);
}

#[test]
fn unmeasured_pass_k_blocks_as_the_core_gate() {
    let mut i = clean_inputs();
    i.pass_k = None; // no agentic data → a Ready verdict would be vacuous
    let v = assess(&i, &lenient());
    assert_eq!(v.status, Readiness::NotReady);
    assert!(v.blocking[0].contains("pass^k not measured"));
}

#[test]
fn epsilon_guard_does_not_falsely_block_a_true_080() {
    let mut i = clean_inputs();
    i.pass_k = Some(0.79999999); // drift below the 0.80 bar, within EPSILON
    let v = assess(&i, &lenient());
    assert!(v.blocking.is_empty(), "drift within epsilon must not block: {:?}", v.blocking);
    assert_eq!(v.status, Readiness::Ready);
}

#[test]
fn forbidden_loop_and_hallucination_each_block() {
    let mut p = lenient();
    p.forbid_infinite_loop = true;
    p.forbid_hallucinated_completion = true;
    let mut i = clean_inputs();
    i.loops = 1;
    i.hallucinated = 2;
    let v = assess(&i, &p);
    assert_eq!(v.status, Readiness::NotReady);
    // The exact affected-run counts are interpolated (loops=1 → singular, hallucinated=2 → plural).
    assert!(v.blocking.contains(&"loops on 1 run".to_string()));
    assert!(v.blocking.contains(&"false 'done' on 2 runs".to_string()));
}

#[test]
fn required_but_unmeasured_memory_is_a_conditional_caveat_not_a_block() {
    // Unmeasured ≠ failure: a required-but-unmeasured memory fit (e.g. llama.cpp /
    // unified memory can't report one) is an honest Conditional caveat, NOT red NotReady.
    let mut p = lenient();
    p.require_full_vram = true;
    let mut i = clean_inputs();
    i.fits_in_vram = None; // never measured
    let v = assess(&i, &p);
    assert_eq!(v.status, Readiness::Conditional);
    assert!(v.blocking.is_empty(), "unmeasured memory must not block");
    assert!(v.conditions.iter().any(|c| c.contains("memory fit not measured")));
}

#[test]
fn measured_partial_offload_blocks_under_require_full_vram() {
    let mut p = lenient();
    p.require_full_vram = true;
    let mut i = clean_inputs();
    i.fits_in_vram = Some(false); // measured: doesn't fit the cap
    let v = assess(&i, &p);
    assert_eq!(v.status, Readiness::NotReady);
    assert!(v.blocking.contains(&"partial offload → severe slowdown".to_string()));
}

#[test]
fn vram_pressure_is_a_conditional_note_independent_of_the_gate() {
    let mut i = clean_inputs(); // require_full_vram off in the lenient profile
    i.vram_pressure = true;
    let v = assess(&i, &lenient());
    assert_eq!(v.status, Readiness::Conditional);
    assert!(v.conditions.contains(&"high VRAM pressure near allocation ceiling".to_string()));
}

#[test]
fn required_but_unmeasured_context_is_a_conditional_caveat_not_a_block() {
    // Same honesty rule for the context-cliff gate: an unprobed model gets a caveat to
    // run the probe, not a red block.
    let mut p = lenient();
    p.min_context_tokens = Some(8192);
    let mut i = clean_inputs();
    i.cliff = CliffStatus::NotProbed;
    let v = assess(&i, &p);
    assert_eq!(v.status, Readiness::Conditional);
    assert!(v.blocking.is_empty(), "unprobed context must not block");
    assert!(v.conditions.iter().any(|c| c.contains("context headroom not measured")));
}

#[test]
fn measured_cliff_below_requirement_blocks_with_math() {
    let mut p = lenient();
    p.min_context_tokens = Some(8192);
    let mut i = clean_inputs();
    i.cliff = CliffStatus::Collapsed { depth: 4096, concentration: None };
    let v = assess(&i, &p);
    assert!(v.blocking.contains(&"reasoning cliff at 4096 < 8192 needed".to_string()));
}

#[test]
fn no_cliff_tested_to_the_requirement_passes() {
    let mut p = lenient();
    p.min_context_tokens = Some(2048);
    let mut i = clean_inputs();
    i.cliff = CliffStatus::NoCliff { tested: 4096, saturated: false }; // held past the requirement
    assert_eq!(assess(&i, &p).status, Readiness::Ready);
}

#[test]
fn no_cliff_probed_short_of_the_requirement_blocks() {
    let mut p = lenient();
    p.min_context_tokens = Some(2048);
    let mut i = clean_inputs();
    i.cliff = CliffStatus::NoCliff { tested: 1024, saturated: false }; // an incomplete probe is not a pass
    let v = assess(&i, &p);
    assert_eq!(v.status, Readiness::NotReady);
    assert!(v.blocking.iter().any(|b| b.contains("only probed to 1024 tok < 2048 needed")));
}

#[test]
fn budget_outcomes_are_conditional_caveats_never_silent_passes() {
    // Both budget verdicts measured the OUTPUT CAP, not context headroom. Before this
    // arm existed, BudgetLimited fell through `_` and silently CERTIFIED headroom the
    // probe never measured — the exact wildcard hazard the Inconclusive arm documents.
    let mut p = lenient();
    p.min_context_tokens = Some(8192);
    for (cliff, needle) in [
        (CliffStatus::BudgetLimited { depth: 6047, cap: 256 }, "budget-limited at 6047 tok"),
        (CliffStatus::CapMarginal { cap: 256, used_milli: 1000 }, "baseline grazed the 256-token output cap"),
    ] {
        let mut i = clean_inputs();
        i.cliff = cliff;
        let v = assess(&i, &p);
        assert_eq!(v.status, Readiness::Conditional, "unmeasured ≠ failure, but never a silent pass");
        assert!(v.blocking.is_empty(), "a budget outcome must not red-block");
        assert!(v.conditions.iter().any(|c| c.contains(needle)), "conditions: {:?}", v.conditions);
    }
}

#[test]
fn broken_baseline_blocks_a_context_gate() {
    let mut p = lenient();
    p.min_context_tokens = Some(2048);
    let mut i = clean_inputs();
    i.cliff = CliffStatus::Broken { tested: 388 }; // fails at the smallest context
    let v = assess(&i, &p);
    assert_eq!(v.status, Readiness::NotReady);
    assert!(v.blocking.iter().any(|b| b.contains("broken") && b.contains("no usable context window")));
}

#[test]
fn slow_latency_is_conditional_not_blocking() {
    let mut p = lenient();
    p.max_ms_per_step = Some(5000);
    let mut i = clean_inputs();
    i.ms_per_step = Some(8400);
    let v = assess(&i, &p);
    assert_eq!(v.status, Readiness::Conditional);
    assert_eq!(v.conditions, vec!["slow: 8400ms/step > 5000ms target".to_string()]);
}

#[test]
fn inefficient_steps_are_conditional() {
    let mut p = lenient();
    p.max_avg_steps = Some(3.0);
    let mut i = clean_inputs();
    i.avg_steps = Some(4.8);
    let v = assess(&i, &p);
    assert_eq!(v.status, Readiness::Conditional);
    assert_eq!(v.conditions, vec!["inefficient: 4.8 avg steps > 3.0 max".to_string()]);
}

#[test]
fn required_native_fc_blocks_when_unsupported() {
    let mut p = lenient();
    p.require_native_fc = true;
    let v = assess(&clean_inputs(), &p); // native_fc is NotSupported
    assert_eq!(v.status, Readiness::NotReady);
    assert!(v.blocking.iter().any(|b| b.contains("native tool-calling required")));
}

#[test]
fn coding_agent_gates_on_measured_vram_fit() {
    let coding = builtins().into_iter().find(|p| p.id == "coding-agent").unwrap();
    let mut overflow = clean_inputs();
    overflow.fits_in_vram = Some(false); // measured: spills past the cap
    assert_eq!(assess(&overflow, &coding).status, Readiness::NotReady);
    let mut fits = clean_inputs();
    fits.fits_in_vram = Some(true);
    assert_eq!(assess(&fits, &coding).status, Readiness::Ready); // clean + fits → Ready
}

#[test]
fn profile_edit_flips_the_verdict_deterministically() {
    let mut i = clean_inputs();
    i.pass_k = Some(0.82);
    let mut p = lenient();
    assert_eq!(assess(&i, &p).status, Readiness::Ready); // 0.82 ≥ 0.80
    p.min_pass_k = 0.90; // raise the bar
    assert_eq!(assess(&i, &p).status, Readiness::NotReady); // 0.82 < 0.90
}

#[test]
fn tier_gate_blocks_when_an_exercised_required_tier_is_not_cleared() {
    let p = ReadinessProfile { required_tier: Tier::Hard, min_pass_k: 0.8, ..lenient() };
    // The collection ran Hard tasks, but the model cleared only Easy (Hard 0.4 < 0.8).
    let i = ReadinessInputs {
        pass_k: Some(0.9), // clears the core gate, so only the tier gate can block
        tier_pass_k: vec![(Tier::Easy, 1.0), (Tier::Hard, 0.4)],
        ..clean_inputs()
    };
    let v = assess(&i, &p);
    assert_eq!(v.status, Readiness::NotReady);
    assert!(v.blocking.iter().any(|b| b.contains("Easy") && b.contains("Hard")), "{:?}", v.blocking);
    // The verdict surfaces the graduated readiness for the report header.
    assert_eq!(v.cleared_tier, Some(Tier::Easy));
    assert_eq!(v.required_tier, Tier::Hard);
}

#[test]
fn tier_gate_is_silent_when_the_required_tier_was_not_exercised() {
    // Only Easy tasks ran → Hard is NotAttempted, never a guessed fail.
    let p = ReadinessProfile { required_tier: Tier::Hard, min_pass_k: 0.8, ..lenient() };
    let i = ReadinessInputs { pass_k: Some(0.9), tier_pass_k: vec![(Tier::Easy, 1.0)], ..clean_inputs() };
    let v = assess(&i, &p);
    assert_eq!(v.status, Readiness::Ready);
    assert!(v.blocking.is_empty(), "{:?}", v.blocking);
}

#[test]
fn tier_gate_passes_when_the_required_tier_is_cleared() {
    let p = ReadinessProfile { required_tier: Tier::Hard, min_pass_k: 0.8, ..lenient() };
    let i = ReadinessInputs {
        pass_k: Some(0.9),
        tier_pass_k: vec![(Tier::Easy, 1.0), (Tier::Hard, 0.9)], // Hard 0.9 ≥ 0.8
        ..clean_inputs()
    };
    let v = assess(&i, &p);
    assert_eq!(v.status, Readiness::Ready);
    assert_eq!(v.cleared_tier, Some(Tier::Hard)); // highest tier cleared at the bar
}

#[test]
fn path_is_taken_from_inputs_not_derived_from_native_fc() {
    // A native-capable model's PROMPT row: native_fc=Tested (model-level capability) but
    // path=PromptBased (the row's own path). The label must follow `path`, not native_fc.
    let i = ReadinessInputs {
        native_fc: NativeFcStatus::Tested { pass_k: 0.9 },
        path: AgentPath::PromptBased,
        ..clean_inputs()
    };
    assert_eq!(assess(&i, &lenient()).path, AgentPath::PromptBased);
}

#[test]
fn require_native_fc_does_not_block_a_native_capable_models_prompt_row() {
    // The prompt-based row of a model that DOES support native (native_fc=Tested) must NOT
    // be blocked under require_native_fc — native is available on the model, this row just
    // measured the prompt path. The gate reads the model-level native_fc, not the path.
    let mut p = lenient();
    p.require_native_fc = true;
    let i = ReadinessInputs {
        native_fc: NativeFcStatus::Tested { pass_k: 0.9 },
        path: AgentPath::PromptBased,
        ..clean_inputs()
    };
    let v = assess(&i, &p);
    assert_eq!(v.status, Readiness::Ready, "{:?}", v.blocking);
    assert_eq!(v.path, AgentPath::PromptBased); // honest label: this row is the prompt path
}

#[test]
fn require_native_fc_still_blocks_a_native_incapable_models_prompt_row() {
    // A model with no native support: native_fc=NotSupported. Its single prompt row is
    // correctly blocked under require_native_fc (native genuinely unavailable).
    let mut p = lenient();
    p.require_native_fc = true;
    let i = ReadinessInputs { native_fc: NativeFcStatus::NotSupported, path: AgentPath::PromptBased, ..clean_inputs() };
    let v = assess(&i, &p);
    assert_eq!(v.status, Readiness::NotReady);
    assert!(v.blocking.iter().any(|b| b.contains("native")), "{:?}", v.blocking);
}

#[test]
fn an_easy_required_tier_never_blocks_on_the_tier_gate() {
    // A pre-Phase-9 profile defaults required_tier=Easy → the gate is inert.
    let p = ReadinessProfile { required_tier: Tier::Easy, ..lenient() };
    let i = ReadinessInputs { tier_pass_k: vec![], ..clean_inputs() };
    assert_eq!(assess(&i, &p).status, Readiness::Ready);
}
