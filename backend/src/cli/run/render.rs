//! Human rendering of a run + the verdict→exit-code mapping, including `--fail-on`.
//!
//! The verdict codes (0/10/20) are the documented QuantaMind contract; `--fail-on`
//! only decides whether a non-Ready verdict fails the *process*, so a team with a
//! soft policy can surface findings without blocking the build.

use super::RunReport;
use crate::cli::doctor::render::label;
use crate::inference::eval::readiness::types::{ModelVerdict, Readiness};

pub const EXIT_READY: i32 = 0;
pub const EXIT_CONDITIONAL: i32 = 10;
/// The run couldn't measure anything (backend fault / timeout) — CI should RETRY.
pub const EXIT_INCONCLUSIVE: i32 = 11;
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

/// Did the run measure nothing? True when no path ran, or every path's trial count
/// is zero — the "couldn't measure → Inconclusive" signal, kept distinct from a
/// measured failure (any positive total_runs, even 0 passes).
pub fn measured_nothing(total_runs: &[u32]) -> bool {
    total_runs.is_empty() || total_runs.iter().all(|&n| n == 0)
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
        let steps = v.avg_steps.map_or("n/a".into(), |a| format!("{a:.1}"));
        out.push_str(&format!(
            "  [{:?}] {}  pass^k={}  runs={}/{}  avg steps={steps}\n",
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
    if let Some(costs) = &r.costs {
        out.push_str(&render_costs(costs));
    }
    out
}

/// Format ms as "1m 5s" / "3.2s" / "850ms".
fn ms(v: u64) -> String {
    if v >= 60_000 {
        format!("{}m {}s", v / 60_000, (v % 60_000) / 1000)
    } else if v >= 1000 {
        format!("{:.1}s", v as f64 / 1000.0)
    } else {
        format!("{v}ms")
    }
}

/// Adaptive byte units — a ~60MB KV cache must never render as "0.0GB" (a real
/// number shown as zero is a fabricated zero).
fn gb(v: u64) -> String {
    if v >= 1_073_741_824 {
        format!("{:.1}GB", v as f64 / 1_073_741_824.0)
    } else if v >= 1_048_576 {
        format!("{:.1}MB", v as f64 / 1_048_576.0)
    } else {
        format!("{:.1}KB", v as f64 / 1024.0)
    }
}

fn opt<T, F: Fn(T) -> String>(v: Option<T>, f: F) -> String {
    v.map(f).unwrap_or_else(|| "n/a".into())
}

/// The `--costs` section: per-(task, pass) cost rows + the run's memory facts — the
/// CLI twin of the app's Latency Test-run view, same honesty rules ("n/a" = the
/// backend reported nothing; "(no split)" = one combined thinking count; RSS is a
/// max of step-END samples of the whole server process).
pub fn render_costs(c: &crate::cli::run::costs::RunCosts) -> String {
    let mut out = String::new();
    out.push_str(&format!("\nRUN COSTS — {}\n", c.model));
    for t in &c.tasks {
        let think = match t.reasoning_tokens_total {
            None => "n/a".into(),
            Some(n) if t.thinking_split_measured => format!("{n} (tokenized split)"),
            Some(n) => format!("{n} (no split)"),
        };
        out.push_str(&format!(
            "  {} [{}]{}  runs={} steps={}  prefill={}  decode={}  out={}  think={}  cache_hits={}  peak_ctx={}  kv@peak={}  wall={}  rss_max={}\n",
            t.task_id,
            t.pass,
            if t.oom { "  ✗ OUT OF MEMORY" } else { "" },
            t.runs,
            t.steps,
            opt(t.prefill_ms_total, ms),
            opt(t.eval_ms_total, ms),
            opt(t.output_tokens_total, |v| v.to_string()),
            think,
            opt(t.cache_hit_tokens_total, |v| format!("{v} tok")),
            opt(t.peak_context_tokens, |v| format!("{v} tok")),
            opt(t.kv_f16_bytes_at_peak, |v| format!("{} (f16)", gb(v))),
            opt(t.wall_ms, ms),
            opt(t.max_step_end_rss_bytes, gb),
        ));
    }
    let m = &c.memory;
    out.push_str(&format!("  model in memory: {}  ({})\n", opt(m.model_bytes, gb), m.model_bytes_provenance));
    if let Some(o) = m.offload_bytes {
        out.push_str(&format!("  spilled to CPU: {} (measured, size − size_vram)\n", gb(o)));
    }
    if let Some(kv) = &m.kv_at_peak {
        out.push_str(&format!(
            "  kv cache @ peak {} tok: f16 {}{} · q8_0 {} · q4_0 {}  (formula{}; cache type is a launch setting)\n",
            kv.peak_tokens,
            gb(kv.f16_bytes),
            if kv.conservative { " (~conservative)" } else { "" },
            gb(kv.q8_0_bytes),
            gb(kv.q4_0_bytes),
            m.kv_cache_type.as_deref().map(|t| format!("; launched {t} KV")).unwrap_or_default(),
        ));
    } else {
        out.push_str("  kv cache @ peak: n/a (model dimensions not measurable)\n");
    }
    if let Some(q) = &m.quantization_claimed {
        out.push_str(&format!("  quantization (tag's claim, unverified): {q}\n"));
    }
    out.push_str("  rss_max = max of step-END samples of the WHOLE server process (weights + residue), never a per-task amount\n");
    out
}

/// Summed `tasks_passed / tasks_total` across a verdict's tiers.
fn tier_totals(v: &ModelVerdict) -> (u32, u32) {
    v.by_tier.iter().fold((0, 0), |(p, t), s| (p + s.tasks_passed, t + s.tasks_total))
}

/// The single largest failure category (`name=count`), or "none" / "—" — for the
/// scoreboard's at-a-glance "what went wrong most".
fn top_failure<T: serde::Serialize>(f: &T) -> String {
    let Ok(serde_json::Value::Object(m)) = serde_json::to_value(f) else {
        return "—".into();
    };
    m.iter()
        .filter_map(|(k, v)| v.as_u64().filter(|&n| n > 0).map(|n| (k.clone(), n)))
        .max_by_key(|(_, n)| *n)
        .map(|(k, n)| format!("{k}={n}"))
        .unwrap_or_else(|| "none".into())
}

/// A per-calling-mode scoreboard (→ stdout) for `qm test`: one row per measured path
/// (native / prompt), with the headline verdict on top. Richer than `render_human`,
/// which is verdict-focused.
pub fn render_scoreboard(r: &RunReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "VERDICT: {}   ({} · {} · {})\n\n",
        status_label(r.worst_status()),
        label(r.backend),
        r.model,
        r.collection_id
    ));
    out.push_str(&format!("{:<14}{:<8}{:<8}{:<7}{:<9}{}\n", "mode", "pass^k", "tasks", "steps", "effort", "top-error"));
    for v in &r.verdicts {
        let (passed, total) = tier_totals(v);
        let tasks = if total > 0 { format!("{passed}/{total}") } else { "—".into() };
        let steps = v.avg_steps.map(|s| format!("{s:.1}")).unwrap_or_else(|| "—".into());
        let effort = v.effort.map(|e| format!("{e:.0}")).unwrap_or_else(|| "—".into());
        out.push_str(&format!(
            "{:<14}{:<8}{:<8}{:<7}{:<9}{}\n",
            format!("{:?}", v.verdict.path),
            passk_str(v.pass_k),
            tasks,
            steps,
            effort,
            top_failure(&v.failures)
        ));
        // A non-Ready mode carries its blocking reasons underneath.
        if v.verdict.status != Readiness::Ready {
            for b in &v.verdict.blocking {
                out.push_str(&format!("    ✗ {b}\n"));
            }
        }
    }
    out.push_str(&format!("\nprofile: {}\n", r.profile_id));
    if let Some(costs) = &r.costs {
        out.push_str(&render_costs(costs));
    }
    out
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
