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
//!   # the server
//!   QM_BACKEND=the server QM_MODEL=llama3.2:3b \
//!     cargo test --test agentic_truncation_sweep -- --ignored --nocapture
//!
//! Env knobs: QM_BACKEND=the server|llama, QM_MODEL=<name>, QM_ENDPOINT=<url>
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
use quantamind_lib::commands::system::hardware::snapshot;
use quantamind_lib::inference::eval::readiness::hardware::hwclass::agentic_ctx_ceiling;
use tokio::sync::mpsc::unbounded_channel;
use tokio_util::sync::CancellationToken;

fn backend() -> BackendKind {
    match std::env::var("QM_BACKEND").unwrap_or_else(|_| "llama".into()).to_lowercase().as_str() {
        "the server" => BackendKind::LlamaCpp,
        "remote" => BackendKind::VLlm,
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

/// §5 sizing: estimate the reasoning tokens in ONE turn's captured `<think>…</think>` block.
/// No model tokenizer here, so ~4 chars/token — an ESTIMATE, honest and adequate for setting a
/// budget WITH margin (cross-checked against the hard fact that these runs never hit finish=length,
/// i.e. reasoning fit under the current budget). `None` when the turn has no captured reasoning.
fn think_est_tokens(raw: &str) -> Option<u32> {
    let start = raw.find("<think>")? + "<think>".len();
    let end = raw[start..].find("</think>").map(|e| start + e).unwrap_or(raw.len());
    let chars = raw[start..end].chars().count();
    (chars > 0).then_some((chars / 4) as u32)
}

/// Nearest-rank percentile of a SORTED slice (empty → 0). Used for the per-tier P50/P95 histogram.
fn percentile(sorted: &[u32], p: f64) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = (((sorted.len() as f64 - 1.0) * p).round() as usize).min(sorted.len() - 1);
    sorted[idx]
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
    // The hardware-adaptive num_ctx ceiling for this machine (the D3 knob). For llama.cpp the eval
    // clamps to the actual launched -c; here we use the class band, matching the live batch path.
    let ctx_ceiling = agentic_ctx_ceiling(snapshot().total_memory_bytes);

    println!("\n================ AGENTIC TRUNCATION SWEEP (k=1) ================");
    println!("backend={backend:?} model={model} endpoint={endpoint} is_thinking={thinking}");
    println!("================================================================\n");

    let mut grand = FailureTracker::default();
    let mut total_tasks = 0u32;
    let mut total_passes = 0u32;
    let mut top_tally: std::collections::BTreeMap<String, u32> = Default::default();
    // §5: per-tier reasoning-token estimates, one entry per turn that emitted a `<think>` block.
    // The distribution (P95) sets `think_tokens_for(tier)` above the chattiest observed turn.
    let mut reasoning_by_tier: std::collections::BTreeMap<String, Vec<u32>> = Default::default();

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
                cpu_offloaded: false,
                ctx_ceiling,
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
            // §5: record each turn's reasoning-token estimate under its tier.
            for s in &steps {
                if let Some(t) = think_est_tokens(&s.raw_output) {
                    reasoning_by_tier.entry(format!("{tier:?}")).or_default().push(t);
                }
            }

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

    // §5: the reasoning-token distribution per tier — the evidence for locking the presets.
    println!("\n============ §5 REASONING-TOKEN HISTOGRAM (est) ============");
    println!("model={model}  (est tokens ≈ <think> chars / 4; one sample per reasoning turn)");
    println!("  {:<9} {:>5} {:>7} {:>7} {:>7}   Standard preset (current)", "tier", "n", "P50", "P95", "max");
    for (tier, vals) in &mut reasoning_by_tier {
        vals.sort_unstable();
        println!(
            "  {:<9} {:>5} {:>7} {:>7} {:>7}",
            tier,
            vals.len(),
            percentile(vals, 0.50),
            percentile(vals, 0.95),
            vals.last().copied().unwrap_or(0),
        );
    }
    println!("Lock rule: think_tokens_for(tier) ≥ P95 × ~1.5 (margin for chattier models than this one).");
    println!("Single-model sample — the population lock needs several reasoning models (see §5).");
    println!("===========================================================\n");

    // Sanity floor: the sweep actually exercised the engine end-to-end.
    assert!(total_tasks > 0, "no tasks ran — collections failed to load");
}
