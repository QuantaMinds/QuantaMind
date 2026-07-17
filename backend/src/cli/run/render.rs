//! Human rendering of a run + the verdict→exit-code mapping, including `--fail-on`.
//!
//! The verdict codes (0/10/20) are the documented QuantaMind contract; `--fail-on`
//! only decides whether a non-Ready verdict fails the *process*, so a team with a
//! soft policy can surface findings without blocking the build.

use super::RunReport;
use crate::cli::doctor::render::label;
use crate::inference::eval::readiness::types::Readiness;

pub const EXIT_READY: i32 = 0;
pub const EXIT_CONDITIONAL: i32 = 10;
pub const EXIT_NOTREADY: i32 = 20;
/// A backend that didn't respond / a missing model — not a failing model.
pub const EXIT_UNREACHABLE: i32 = 3;

/// Which verdicts fail the *process*. The verdict itself is always reported; this
/// only gates the exit status for CI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailOn {
    /// Default: Conditional → 10, NotReady → 20.
    Conditional,
    /// Tolerate Conditional (exit 0), still fail on NotReady.
    NotReady,
    /// Advisory-only: always exit 0.
    Never,
}

/// Map the run's worst verdict + the fail-on policy to the process exit code.
pub fn exit_code(status: Readiness, fail_on: FailOn) -> i32 {
    match fail_on {
        FailOn::Never => EXIT_READY,
        FailOn::NotReady => match status {
            Readiness::NotReady => EXIT_NOTREADY,
            _ => EXIT_READY, // Ready and Conditional both pass a soft policy
        },
        FailOn::Conditional => match status {
            Readiness::Ready => EXIT_READY,
            Readiness::Conditional => EXIT_CONDITIONAL,
            Readiness::NotReady => EXIT_NOTREADY,
        },
    }
}

/// Parse a 1-based menu selection into a 0-based index, rejecting anything out of
/// `1..=n` (and blank/non-numeric). Pure so the interactive picker's logic is tested
/// without a TTY.
pub fn parse_selection(input: &str, n: usize) -> Option<usize> {
    let choice: usize = input.trim().parse().ok()?;
    (1..=n).contains(&choice).then(|| choice - 1)
}

fn status_label(s: Readiness) -> &'static str {
    match s {
        Readiness::Ready => "Ready",
        Readiness::Conditional => "Conditional",
        Readiness::NotReady => "Not Ready",
    }
}

fn passk_str(p: Option<f64>) -> String {
    p.map(|v| format!("{v:.2}")).unwrap_or_else(|| "N/A".into())
}

/// The run report as human text (→ stdout). Shows the headline verdict, then each
/// measured path with its strict pass^k, run tally, and the exact blocking (`✗`) /
/// soft (`!`) reasons the verdict gated on.
pub fn render_human(r: &RunReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "VERDICT: {}   ({} · {} · {})\n",
        status_label(r.worst_status()),
        label(r.backend),
        r.model,
        r.collection_id
    ));
    for v in &r.verdicts {
        out.push_str(&format!(
            "  [{:?}] {}  pass^k={}  runs={}/{}\n",
            v.verdict.path,
            status_label(v.verdict.status),
            passk_str(v.pass_k),
            v.passes,
            v.total_runs
        ));
        for b in &v.verdict.blocking {
            out.push_str(&format!("    ✗ {b}\n"));
        }
        for c in &v.verdict.conditions {
            out.push_str(&format!("    ! {c}\n"));
        }
    }
    out.push_str(&format!("\nprofile: {}\n", r.profile_id));
    out
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
