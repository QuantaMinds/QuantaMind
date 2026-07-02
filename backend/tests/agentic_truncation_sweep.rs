//! LIVE truncation-fix sweep. Runs EVERY bundled v2 collection (all coding /
//! finance / medical / … tiers) at k=1 against a real backend and prints the
//! per-task outcome + an aggregate failure-kind histogram. The point is to see
//! the PATTERN the token-budget-truncation fix produces: with the fix, a turn
//! cut off at the `num_predict` cap is either recovered by the context-clamped
//! retry (→ pass) or labeled `Truncated` honestly — it must NEVER be laundered
//! into Malformed / Hallucinated / EmptyOutput.
//!
//! `#[ignore]` — needs a live server + a real model. Run per backend:
//!
//!   # llama.cpp (server already up on :8081, model loaded there)
//!   QM_BACKEND=llama QM_MODEL=qwen3.5-9b_q8_0 \
//!     cargo test --test agentic_truncation_sweep -- --ignored --nocapture
//!
//!   # Ollama
//!   QM_BACKEND=ollama QM_MODEL=llama3.2:3b \
//!     cargo test --test agentic_truncation_sweep -- --ignored --nocapture
//!
//! Env knobs: QM_BACKEND=ollama|llama, QM_MODEL=<name>, QM_ENDPOINT=<url>
//! (defaults to the backend's standard port), QM_IS_THINKING=1 to raise the
//! per-turn budget by the reasoning scratchpad.

use quantamind_lib::inference::backend::backend_kind::BackendKind;
use quantamind_lib::inference::backend::endpoint;
use quantamind_lib::inference::eval::agentic::build::sandbox_for;
use quantamind_lib::inference::eval::agentic::difficulty::passk::max_tokens_for;
use quantamind_lib::inference::eval::agentic::model_turn::BackendTurn;
use quantamind_lib::inference::eval::agentic::runner::run_agentic;
use quantamind_lib::inference::eval::agentic::scoring::report::{AgenticReport, FailureTracker};
use quantamind_lib::inference::eval::agentic::spec::Tier;
use quantamind_lib::inference::eval::agentic::step::{StepKind, TrajectoryStep};
use quantamind_lib::inference::eval::agentic::v2::collection::load_v2_collection;
use quantamind_lib::inference::eval::agentic::v2::scenarios::V2_SCENARIOS;
use tokio::sync::mpsc::unbounded_channel;
use tokio_util::sync::CancellationToken;

fn backend() -> BackendKind {
    match std::env::var("QM_BACKEND").unwrap_or_else(|_| "llama".into()).to_lowercase().as_str() {
        "ollama" => BackendKind::Ollama,
        _ => BackendKind::LlamaCpp,
    }
}

fn model() -> String {
    std::env::var("QM_MODEL").unwrap_or_else(|_| "qwen3.5-9b_q8_0".into())
}

fn endpoint_for(b: BackendKind) -> String {
    std::env::var("QM_ENDPOINT").unwrap_or_else(|_| endpoint::default_for(b).to_string())
}

fn is_thinking() -> bool {
    matches!(std::env::var("QM_IS_THINKING").ok().as_deref(), Some("1") | Some("true"))
}

fn tier_of(task: &quantamind_lib::inference::eval::toolcall::tasks::ToolTask) -> Tier {
    task.agentic.as_ref().map(|a| a.tier).unwrap_or_default()
}

/// Collapse whitespace/newlines and truncate to `n` chars so a multi-line model turn
/// prints as one readable line in the trajectory dump.
fn snip(s: &str, n: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > n {
        format!("{}…", one.chars().take(n).collect::<String>())
    } else {
        one
    }
}

/// Every failure bucket the tracker holds, as `(label, count)` — printed as a
/// histogram so a spike in `truncated` (recovered/labeled) vs `malformed` /
/// `hallucinated` (laundered) is visible at a glance.
fn buckets(f: &FailureTracker) -> Vec<(&'static str, u32)> {
    vec![
        ("forbidden", f.forbidden_calls),
        ("turn_timeout", f.turn_timeouts),
        ("infinite_loop", f.infinite_loop_hits),
        ("hallucinated", f.hallucinated_completions),
        ("malformed_schema", f.schema_unrecovered_calls),
        ("malformed_json", f.malformed_json_calls),
        ("foreign_dialect", f.foreign_dialect_calls),
        ("truncated", f.truncated_calls),
        ("empty_output", f.empty_output_calls),
        ("reported_in_prose", f.reported_in_prose_calls),
        ("unknown_tool(diag)", f.unknown_tool_calls),
    ]
}

/// Field-wise accumulate (the tracker's own `merge` is `pub(crate)`, unreachable
/// from an integration test).
fn add_into(g: &mut FailureTracker, o: &FailureTracker) {
    g.forbidden_calls += o.forbidden_calls;
    g.turn_timeouts += o.turn_timeouts;
    g.infinite_loop_hits += o.infinite_loop_hits;
    g.hallucinated_completions += o.hallucinated_completions;
    g.schema_unrecovered_calls += o.schema_unrecovered_calls;
    g.malformed_json_calls += o.malformed_json_calls;
    g.foreign_dialect_calls += o.foreign_dialect_calls;
    g.truncated_calls += o.truncated_calls;
    g.empty_output_calls += o.empty_output_calls;
    g.reported_in_prose_calls += o.reported_in_prose_calls;
    g.unknown_tool_calls += o.unknown_tool_calls;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn sweep_all_collections_k1() {
    let backend = backend();
    let model = model();
    let endpoint = endpoint_for(backend);
    let thinking = is_thinking();

    println!("\n================ AGENTIC TRUNCATION SWEEP (k=1) ================");
    println!("backend={backend:?} model={model} endpoint={endpoint} is_thinking={thinking}");
    println!("================================================================\n");

    let mut grand = FailureTracker::default();
    let mut total_tasks = 0u32;
    let mut total_passes = 0u32;
    let mut top_tally: std::collections::BTreeMap<String, u32> = Default::default();

    // Optional substring filter (QM_ONLY=coding runs only matching collections) — lets a
    // smoke run hit one small collection before the full multi-hour sweep.
    let only = std::env::var("QM_ONLY").ok();
    // QM_TRACE=1 dumps each task's prompt + per-step kind/raw/injection so the REASON behind
    // every pass/fail is inspectable (not just the top-line kind).
    let trace = matches!(std::env::var("QM_TRACE").ok().as_deref(), Some("1") | Some("true"));

    for (cid, json) in V2_SCENARIOS {
        if let Some(sub) = &only {
            if !cid.contains(sub.as_str()) {
                continue;
            }
        }
        let tasks = match load_v2_collection(json) {
            Ok(t) => t,
            Err(e) => {
                println!("[{cid}] SKIP load error: {e}");
                continue;
            }
        };
        println!("── collection {cid}  ({} tasks) ──────────────────────────", tasks.len());
        for task in &tasks {
            let tier = tier_of(task);
            let max_tokens = max_tokens_for(tier, thinking);
            let turn = BackendTurn {
                backend,
                endpoint: endpoint.clone(),
                model: model.clone(),
                cancel: CancellationToken::new(),
                options: None,
                keep_alive: None,
                is_thinking: thinking,
                max_tokens,
                stop_cache: Default::default(),
            };
            let (sandbox, mut cfg) = match sandbox_for(task) {
                Ok(v) => v,
                Err(e) => {
                    println!("  {:<28} SANDBOX ERROR: {e}", task.id);
                    continue;
                }
            };
            cfg.k = 1; // one iteration, as requested

            let (tx, mut rx) = unbounded_channel::<TrajectoryStep>();
            let mut steps: Vec<TrajectoryStep> = Vec::new();
            let report: Result<AgenticReport, _> = run_agentic(&turn, &sandbox, cfg, &tx).await;
            drop(tx);
            while let Ok(s) = rx.try_recv() {
                steps.push(s);
            }
            let kinds: Vec<StepKind> = steps.iter().map(|s| s.kind.clone()).collect();

            total_tasks += 1;
            match report {
                Ok(r) => {
                    add_into(&mut grand, &r.failures);
                    let passed = r.passes > 0;
                    if passed {
                        total_passes += 1;
                    }
                    *top_tally.entry(format!("{:?}", r.top_error)).or_default() += 1;
                    let trunc = r.failures.truncated_calls;
                    let flag = if trunc > 0 { "  <-- TRUNCATED" } else { "" };
                    println!(
                        "  {:<28} {:<7} tier={:?} pred={} top={:?} steps={:?} tok={:?} kinds={:?}{}",
                        task.id,
                        if passed { "PASS" } else { "FAIL" },
                        tier,
                        max_tokens,
                        r.top_error,
                        r.avg_steps,
                        r.avg_output_tokens_success,
                        kinds,
                        flag,
                    );
                    if trace {
                        println!("      PROMPT: {}", snip(&task.prompt, 240));
                        for s in &steps {
                            let inj = s.injection.as_deref().map(|i| format!("  INJECT<{}>", snip(i, 120))).unwrap_or_default();
                            let fr = if matches!(s.kind, StepKind::Truncated) { " [finish=length]" } else { "" };
                            println!(
                                "      #{} {:?}{}{}  raw=\"{}\"",
                                s.step_index,
                                s.kind,
                                fr,
                                inj,
                                snip(&s.raw_output, 300),
                            );
                        }
                    }
                }
                Err(e) => {
                    *top_tally.entry("Error".into()).or_default() += 1;
                    println!("  {:<28} ERROR  tier={:?} pred={} : {e}", task.id, tier, max_tokens);
                }
            }
        }
        println!();
    }

    println!("==================== AGGREGATE PATTERN =====================");
    println!("backend={backend:?} model={model} is_thinking={thinking}");
    println!("tasks={total_tasks}  passes={total_passes}  fails={}", total_tasks - total_passes);
    println!("\n-- top_error tally (headline per task) --");
    for (k, v) in &top_tally {
        println!("  {k:<20} {v}");
    }
    println!("\n-- failure-kind histogram (summed over all runs) --");
    for (label, count) in buckets(&grand) {
        if count > 0 {
            println!("  {label:<20} {count}");
        }
    }
    println!("\nKEY: `truncated` = honestly-labeled cap hit (the fix working). A NON-zero");
    println!("`malformed_json` / `hallucinated` / `empty_output` on a batched-write task is");
    println!("the laundering the fix is meant to eliminate — cross-check those trajectories.");
    println!("===========================================================\n");

    // Sanity floor: the sweep actually exercised the engine end-to-end.
    assert!(total_tasks > 0, "no tasks ran — collections failed to load");
}
