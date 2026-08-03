//! Human output + the verdict→exit-code mapping for `qm certify`.

use crate::cli::certify::{CertifyOutcome, CertifyReport};
use crate::cli::run::render::{
    exit_code, FailOn, EXIT_INCONCLUSIVE, EXIT_NOTREADY, EXIT_READY, EXIT_UNREACHABLE,
};
use crate::inference::eval::harness::AttemptStatus;

/// Map a run to its process exit code.
///
/// Reuses the shared `exit_code` contract with one override: **an unmeasured
/// attempt reports 11 Inconclusive rather than a verdict.** This is an attribution
/// rule, not just a severity one. A missing `sqlite3` or an unstartable command is
/// *our* problem; letting it surface as 10/Conditional would tell the user their
/// agent is flaky when in fact we never measured it. 11 means "retry", which is
/// the honest instruction.
///
/// A **proven hard failure outranks it**: if some task failed every attempt, that
/// is a real, observed verdict and stays 20. Same precedent as
/// `cli/validate.rs::run_validate`, where static findings outrank an inconclusive
/// live check.
///
/// `--fail-on never` is honoured first — an explicit, printed operator decision to
/// treat the run as advisory.
pub fn certify_exit_code(report: &CertifyReport, fail_on: FailOn) -> i32 {
    if fail_on == FailOn::Never {
        return EXIT_READY;
    }
    // The command never started, not once. That is a configuration error — a
    // misspelled program, a missing interpreter — and retrying will not fix it, so
    // it must not report 11 ("retry"). 3 is the existing "system under test is not
    // runnable" code, the same meaning `doctor` gives an unreachable backend.
    if report.never_started() {
        return EXIT_UNREACHABLE;
    }
    let base = exit_code(report.verdict(), fail_on);
    if report.inconclusive() && base != EXIT_NOTREADY {
        return EXIT_INCONCLUSIVE;
    }
    base
}

/// Render the human report to stdout. `[QM-…]` notes go to stderr so `--json`
/// stays pipeable.
pub fn render(outcome: &CertifyOutcome, fail_on: FailOn) -> i32 {
    match outcome {
        CertifyOutcome::BadSuite(msg) => {
            eprintln!("[QM-BAD-SUITE] {msg}");
            2
        }
        CertifyOutcome::NotDiscriminating { task_id } => {
            eprintln!(
                "[QM-NOT-DISCRIMINATING] task '{task_id}' is vacuous — a do-nothing agent passes \
                 it, so it can never fail and would silently green-light every future run. No \
                 agent was started."
            );
            EXIT_NOTREADY
        }
        CertifyOutcome::Ran(r) => {
            println!("VERDICT: {:?}", r.verdict());
            println!();
            for t in &r.tasks {
                let mark = if t.is_strict_pass() { "PASS" } else { "FAIL" };
                let median = t
                    .median_wall_ms()
                    .map(|m| format!("{:.1}s", m as f64 / 1000.0))
                    // `None` means not measured. Never print 0.0s.
                    .unwrap_or_else(|| "n/a".into());
                println!("  {mark}  {}  {}/{}  median {median}", t.task_id, t.passes(), t.k);
                for a in t.attempts.iter().filter(|a| !a.status.is_pass()) {
                    println!("      attempt {}  {}", a.n, a.status.label());
                    for line in a.stderr_tail.iter().rev().take(3).rev() {
                        println!("          | {line}");
                    }
                }
            }

            // Totals name EVERY class, including the ones that aren't verdicts —
            // silent exclusion is the same bug as silent inclusion.
            let all = r.tasks.iter().flat_map(|t| &t.attempts);
            let (mut pass, mut state, mut exit, mut to, mut spawn, mut harness) = (0, 0, 0, 0, 0, 0);
            for a in all {
                match a.status {
                    AttemptStatus::Passed => pass += 1,
                    AttemptStatus::FailedState { .. } => state += 1,
                    AttemptStatus::AgentExitNonZero { .. } => exit += 1,
                    AttemptStatus::AgentTimeout { .. } => to += 1,
                    AttemptStatus::AgentSpawnFailed { .. } => spawn += 1,
                    AttemptStatus::HarnessError { .. } => harness += 1,
                }
            }
            println!();
            println!(
                "{} tasks · {} passed · {state} wrong-state · {exit} non-zero exit · {to} timeout \
                 · {spawn} could-not-start · {harness} harness error",
                r.tasks.len(),
                pass
            );
            println!("agent: {}   (template as configured)", r.command_template);
            println!("wall-clock only — QuantaMind does not observe your agent's model spend.");

            if r.one_sided {
                eprintln!(
                    "[QM-ONE-SIDED] every task in this suite rewards action. A one-sided suite \
                     teaches over-triggering; add a `negative` task where the correct answer is \
                     to refuse."
                );
            }
            if r.never_started() {
                eprintln!(
                    "[QM-AGENT-UNREACHABLE] the agent command never started on any attempt — \
                     check the program path. Retrying will not help."
                );
            } else if r.inconclusive() {
                eprintln!(
                    "[QM-INCONCLUSIVE] at least one attempt could not be measured — this run \
                     cannot be green regardless of the other results."
                );
            }
            let code = certify_exit_code(r, fail_on);
            if fail_on == FailOn::Never && r.verdict() != crate::inference::eval::readiness::types::Readiness::Ready {
                eprintln!("[QM-NOTE] verdict is {:?} but --fail-on never let it pass (exit 0)", r.verdict());
            }
            code
        }
    }
}

/// Write a recorded suite to disk.
///
/// Only tasks whose agent actually changed the world are emitted. A task with an
/// empty delta is skipped **loudly**: recording it would produce an oracle that
/// asserts nothing, i.e. a test that can never fail — the exact defect the
/// anti-vacuity gate exists to catch. Silently writing it would smuggle a vacuous
/// task into the suite through the back door.
pub fn write_recorded(
    report: &CertifyReport,
    tasks: &[crate::cli::certify::suite::CertifyTask],
    out: &std::path::Path,
) -> Result<usize, String> {
    use crate::cli::certify::record;

    let mut emitted = Vec::new();
    for (id, delta) in &report.recorded {
        let Some(t) = tasks.iter().find(|t| &t.id == id) else { continue };
        if let Some(why) = record::unsupported_reason(&t.spec) {
            eprintln!("[QM-RECORD-SKIP] '{id}': {why}");
            continue;
        }
        if delta.is_empty() {
            // Two different things, and saying the wrong one sends the user to
            // debug the wrong problem. "Changed nothing" and "only edited bodies"
            // both yield no structural assertion, but only the first means the
            // agent did nothing.
            if delta.modified.is_empty() {
                eprintln!(
                    "[QM-RECORD-SKIP] '{id}': the agent changed nothing — no file was created or \
                     deleted, so there is no assertion to record. A recorded oracle here would \
                     assert nothing and could never fail."
                );
            } else {
                eprintln!(
                    "[QM-RECORD-SKIP] '{id}': the agent only MODIFIED existing files ({}) and \
                     created or deleted none. Content assertions are not auto-generated — a \
                     recorded body embeds run-specific text — so there is nothing to record here. \
                     Add `assert_content` by hand for those files.",
                    delta.modified.join(", ")
                );
            }
            continue;
        }
        if !delta.modified.is_empty() {
            eprintln!(
                "[QM-RECORD-REVIEW] '{id}': {} file(s) were MODIFIED — content assertions are not \
                 auto-generated (a recorded body embeds run-specific text). Add assert_content for: {}",
                delta.modified.len(),
                delta.modified.join(", ")
            );
        }
        // Start from the ORIGINAL task and replace only the oracle, so the world
        // (and any field this loader does not model) survives verbatim. Rebuilding
        // it from our own types would silently drop anything we do not parse.
        let mut task = t.source.clone();
        if !task.is_object() {
            task = serde_json::json!({ "name": id, "instruction": t.goal, "k": t.k });
        }
        task["oracle"] = record::to_oracle_json(delta);
        if !delta.modified.is_empty() {
            task["_modified"] = serde_json::json!(delta.modified);
        }
        emitted.push(task);
    }

    if emitted.is_empty() {
        return Err("nothing was recorded — see the [QM-RECORD-SKIP] notes above".into());
    }
    let doc = serde_json::json!({ "_banner": record::REVIEW_BANNER, "tasks": emitted });
    let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    std::fs::write(out, text)
        .map_err(|e| crate::redact::redact_path(&e.to_string()))?;
    Ok(emitted.len())
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
