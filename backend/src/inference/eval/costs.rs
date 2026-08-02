//! Wall-clock seconds → dollars for a batch run.
//!
//! No new measurement happens here: the runner already stamps each attempt's
//! wall-clock and how it ended (`AttemptCost`); this module only applies a
//! user-declared price. The rules that matter are which attempts may be priced
//! at all, and saying out loud what was left out.
//!
//! **There is no default price.** Absent one, every USD field is `None` and the
//! report reads "n/a (no price basis)" — never `$0.00`. A fabricated price is
//! worse than no price: it silently understates a real bill.
//!
//! **The figure is an upper bound.** The bench runs serially with the accelerator
//! to itself; production batches concurrent requests and amortizes the same fixed
//! cost across them. `basis_note` states that in the output rather than leaving the
//! reader to discover it.

use crate::inference::eval::agentic::scoring::report::{AgenticReport, AttemptClass};
use serde::{Deserialize, Serialize};

/// User-declared pricing. Nothing here has a default price — see the module note.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct CostConfig {
    pub enabled: bool,
    /// Hourly rate for the accelerator this ran on. `None` ⇒ no price basis, so
    /// every USD figure is `None`.
    pub gpu_hourly_usd: Option<f64>,
    /// Fraction of the accelerator this run actually had. 1.0 = the whole card.
    pub utilization: f64,
}

impl Default for CostConfig {
    fn default() -> Self {
        Self { enabled: false, gpu_hourly_usd: None, utilization: 1.0 }
    }
}

impl CostConfig {
    /// Dollars per wall-clock second, or `None` when there's no usable basis.
    /// A non-positive or non-finite price/utilization is treated as ABSENT rather
    /// than propagated: it would otherwise produce a `$0.00` or infinite bill that
    /// reads like a measurement.
    fn usd_per_second(&self) -> Option<f64> {
        let hourly = self.gpu_hourly_usd.filter(|h| h.is_finite() && *h > 0.0)?;
        let util = if self.utilization.is_finite() && self.utilization > 0.0 { self.utilization } else { return None };
        Some(hourly / 3600.0 / util)
    }
}

/// The costed result for one run. Every USD field is `Option`: `None` means "not
/// measured / no basis", which the renderers print as `n/a` — never 0.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct RunCostSummary {
    /// Always `"gpu_seconds"` — names WHAT was priced, so a later token-based
    /// basis can't be confused for this one.
    pub basis: &'static str,
    /// Mandatory prose stating the price, the utilization, and that serial
    /// wall-clock is an UPPER bound. Emitted even when there's no price.
    pub basis_note: String,
    pub cost_per_attempt_usd: Option<f64>,
    pub cost_per_task_usd: Option<f64>,
    pub cost_per_success_usd: Option<f64>,
    pub run_total_usd: Option<f64>,
    /// Attempts left out of every figure: died at the cap, or passed but were cut
    /// at it. Printed, never silent — a silent exclusion misleads exactly as much
    /// as a silent inclusion.
    pub excluded_truncated: usize,
    /// False when there was no price basis, or nothing priceable ran.
    pub cost_measured: bool,
}

impl RunCostSummary {
    /// The no-basis result: exclusions still counted, every dollar figure absent.
    fn unpriced(note: String, excluded: usize) -> Self {
        Self {
            basis: "gpu_seconds",
            basis_note: note,
            cost_per_attempt_usd: None,
            cost_per_task_usd: None,
            cost_per_success_usd: None,
            run_total_usd: None,
            excluded_truncated: excluded,
            cost_measured: false,
        }
    }
}

/// One task's cost inputs: its attempts, and whether it cleared pass^k.
pub struct TaskCostInput<'a> {
    pub report: &'a AgenticReport,
    /// Did this task meet the pass^k bar? Supplied by the caller — the threshold
    /// is the eval engine's business, not this module's.
    pub meets_pass_k: bool,
}

/// Median of an already-collected sample. `None` for an empty sample — a median
/// of nothing is not zero.
fn median(mut xs: Vec<f64>) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).expect("costs are finite"));
    let mid = xs.len() / 2;
    Some(if xs.len() % 2 == 0 { (xs[mid - 1] + xs[mid]) / 2.0 } else { xs[mid] })
}

/// Whether an attempt's time may be priced at all.
///
/// A cap-ended attempt is excluded from EVERY figure: pricing a run the harness
/// cut short quotes a cheap number for a broken result — the contamination this
/// whole surface exists to avoid.
fn priceable(class: AttemptClass) -> bool {
    matches!(class, AttemptClass::PassedClean | AttemptClass::FailedContent)
}

/// Cost a whole run. `tasks` is every task in the run; `cfg` the declared price.
///
/// Which attempts land where:
///
/// | attempt         | per-attempt median | per-task median | success denominator |
/// |-----------------|--------------------|-----------------|---------------------|
/// | passed clean    | in                 | in              | task can count      |
/// | failed on content | out              | in              | task doesn't count  |
/// | died at cap     | out                | out             | excluded + counted  |
/// | truncated pass  | out                | out             | excluded + counted  |
///
/// A failed attempt stays in the task total and in `run_total` because the retry
/// cost real money — that IS the number the bill reflects. It's kept out of the
/// per-attempt median so that figure stays "what a working attempt costs".
pub fn summarize(tasks: &[TaskCostInput<'_>], cfg: &CostConfig) -> RunCostSummary {
    let excluded: usize = tasks
        .iter()
        .flat_map(|t| t.report.attempts.iter())
        .filter(|a| !priceable(a.class))
        .count();

    let Some(usd_per_sec) = cfg.usd_per_second() else {
        return RunCostSummary::unpriced(note(cfg), excluded);
    };
    let secs = |ms: u64| ms as f64 / 1000.0;

    // Per-attempt median: clean passes only — "what a working attempt costs".
    // A missing wall-clock makes the whole sample unmeasured rather than a median
    // over the attempts that happen to carry a time.
    let mut clean: Vec<f64> = Vec::new();
    let mut clean_unmeasured = false;
    for a in tasks.iter().flat_map(|t| t.report.attempts.iter()) {
        if a.class != AttemptClass::PassedClean {
            continue;
        }
        match a.wall_ms {
            Some(ms) => clean.push(secs(ms) * usd_per_sec),
            None => clean_unmeasured = true,
        }
    }
    let cost_per_attempt_usd = if clean_unmeasured { None } else { median(clean) };

    // Per-task total: every priceable attempt for that task, summed. A task whose
    // attempts were ALL excluded contributes no total (not a zero — it has no
    // priceable time, which is different from being free).
    let mut task_totals: Vec<f64> = Vec::new();
    let mut task_unmeasured = false;
    let mut run_total = 0.0f64;
    let mut run_total_unmeasured = false;
    for t in tasks {
        let mut sum = 0.0f64;
        let mut counted = 0usize;
        let mut missing = false;
        for a in t.report.attempts.iter().filter(|a| priceable(a.class)) {
            match a.wall_ms {
                Some(ms) => {
                    sum += secs(ms) * usd_per_sec;
                    counted += 1;
                }
                None => missing = true,
            }
        }
        if missing {
            task_unmeasured = true;
            run_total_unmeasured = true;
            continue;
        }
        if counted > 0 {
            task_totals.push(sum);
            run_total += sum;
        }
    }
    let cost_per_task_usd = if task_unmeasured { None } else { median(task_totals) };
    let run_total_usd = if run_total_unmeasured { None } else { Some(run_total) };

    // Per success: the WHOLE run's bill over the tasks that actually cleared
    // pass^k. Zero such tasks ⇒ `None`, never an infinity — a run that succeeded
    // at nothing has no cost-per-success, it has a cost and no successes.
    let successes = tasks.iter().filter(|t| t.meets_pass_k).count();
    let cost_per_success_usd = match (run_total_usd, successes) {
        (Some(total), n) if n > 0 => Some(total / n as f64),
        _ => None,
    };

    RunCostSummary {
        basis: "gpu_seconds",
        basis_note: note(cfg),
        cost_per_attempt_usd,
        cost_per_task_usd,
        cost_per_success_usd,
        run_total_usd,
        excluded_truncated: excluded,
        cost_measured: run_total_usd.is_some(),
    }
}

/// The mandatory basis line. States the price and utilization, and that a serial
/// benchmark is an UPPER bound on a batched deployment — the error bar a reader
/// would otherwise have to guess at.
fn note(cfg: &CostConfig) -> String {
    match cfg.usd_per_second() {
        None => "no price basis: set costs.gpu_hourly_usd to price this run".to_string(),
        Some(_) => format!(
            "serial wall-clock at ${:.2}/hr, utilization={:.2}. Upper bound: production batching \
             amortizes fixed cost across concurrent requests.",
            cfg.gpu_hourly_usd.unwrap_or(0.0),
            cfg.utilization,
        ),
    }
}

/// The single CLI line. `n/a` whenever a figure has no basis — never `$0.00`.
pub fn cli_line(c: &RunCostSummary) -> String {
    let usd = |v: Option<f64>| match v {
        Some(x) => format!("{x:.3}"),
        None => "n/a".to_string(),
    };
    if !c.cost_measured {
        return format!("$/task  n/a (no price basis)  ·  {} runs excluded (died at cap)", c.excluded_truncated);
    }
    format!(
        "$/task  {}   ·  $/success  {}   ·  {} runs excluded (died at cap)",
        usd(c.cost_per_task_usd),
        usd(c.cost_per_success_usd),
        c.excluded_truncated,
    )
}

#[cfg(test)]
#[path = "costs_tests.rs"]
mod tests;
