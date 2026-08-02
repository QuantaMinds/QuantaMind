use super::*;
use crate::inference::eval::agentic::scoring::report::AttemptCost;

const PRICE: f64 = 0.98;
/// 0.98 / 3600 — the per-second rate every expected figure below is built from.
const PER_SEC: f64 = PRICE / 3600.0;

fn cfg() -> CostConfig {
    CostConfig { enabled: true, gpu_hourly_usd: Some(PRICE), utilization: 1.0 }
}

fn attempt(secs: u64, class: AttemptClass) -> AttemptCost {
    AttemptCost { wall_ms: Some(secs * 1000), class }
}

/// A task report carrying only what the cost layer reads (`from_outcomes` builds
/// the rest; the cost layer never looks at it).
fn task(attempts: Vec<AttemptCost>) -> AgenticReport {
    let mut r = AgenticReport::from_outcomes(&[]);
    r.attempts = attempts;
    r
}

fn close(got: Option<f64>, want: f64) {
    let g = got.expect("expected a priced figure, got None");
    assert!((g - want).abs() < 1e-9, "got {g}, want {want}");
}

/// The hand-computed reference case (worked on paper first, then asserted here):
/// 5 tasks, k=3, $0.98/hr, utilization 1.0. T4 dies at the cap throughout; T5
/// has one truncated pass. Only T1 clears pass^k.
fn reference() -> Vec<AgenticReport> {
    use AttemptClass::*;
    vec![
        task(vec![attempt(10, PassedClean), attempt(10, PassedClean), attempt(10, PassedClean)]),
        task(vec![attempt(20, FailedContent), attempt(20, FailedContent), attempt(20, PassedClean)]),
        task(vec![attempt(30, FailedContent), attempt(30, FailedContent), attempt(30, FailedContent)]),
        task(vec![attempt(40, DiedAtCap), attempt(40, DiedAtCap), attempt(40, DiedAtCap)]),
        task(vec![attempt(50, PassedClean), attempt(50, TruncatedPass), attempt(50, PassedClean)]),
    ]
}

fn inputs(reports: &[AgenticReport], meets: [bool; 5]) -> Vec<TaskCostInput<'_>> {
    reports.iter().zip(meets).map(|(report, meets_pass_k)| TaskCostInput { report, meets_pass_k }).collect()
}

#[test]
fn reference_case_matches_the_hand_computed_figures() {
    let reports = reference();
    // Only T1 passes all 3 runs. T5 is 2/3 — a truncated pass is not a clean pass.
    let c = summarize(&inputs(&reports, [true, false, false, false, false]), &cfg());

    // Per-attempt: clean attempts are [10,10,10,20,50,50]s → median 15s.
    close(c.cost_per_attempt_usd, 15.0 * PER_SEC);
    // Per-task priceable sums: T1 30, T2 60, T3 90, T5 100 (T4 has none) → median 75s.
    close(c.cost_per_task_usd, 75.0 * PER_SEC);
    // Run total: 30+60+90+100 = 280s of priceable time.
    close(c.run_total_usd, 280.0 * PER_SEC);
    // One task cleared pass^k, so cost-per-success is the whole bill.
    close(c.cost_per_success_usd, 280.0 * PER_SEC);
    // 3 cap deaths (T4) + 1 truncated pass (T5).
    assert_eq!(c.excluded_truncated, 4);
    assert!(c.cost_measured);
}

/// The product argument: a model can look cheaper per task and cost far more per
/// correct answer. If these two figures ever collapse into one, the surface has
/// lost its point.
#[test]
fn per_task_and_per_success_diverge_when_a_model_is_unreliable() {
    use AttemptClass::*;
    // Cheap per attempt, but only one task in four actually clears the bar.
    let reports: Vec<AgenticReport> = (0..4)
        .map(|_| task(vec![attempt(10, PassedClean), attempt(10, FailedContent)]))
        .collect();
    let ins: Vec<TaskCostInput<'_>> = reports
        .iter()
        .enumerate()
        .map(|(i, report)| TaskCostInput { report, meets_pass_k: i == 0 })
        .collect();
    let c = summarize(&ins, &cfg());

    close(c.cost_per_task_usd, 20.0 * PER_SEC); // every task cost 20s
    close(c.cost_per_success_usd, 80.0 * PER_SEC); // the whole 80s bought ONE success
    assert!(
        c.cost_per_success_usd.unwrap() > c.cost_per_task_usd.unwrap(),
        "unreliability must show up as a higher price per correct answer"
    );
}

#[test]
fn no_price_yields_all_nulls_and_cost_measured_false() {
    let reports = reference();
    let cfg = CostConfig { enabled: true, gpu_hourly_usd: None, utilization: 1.0 };
    let c = summarize(&inputs(&reports, [true, false, false, false, false]), &cfg);

    assert_eq!(c.cost_per_attempt_usd, None);
    assert_eq!(c.cost_per_task_usd, None);
    assert_eq!(c.cost_per_success_usd, None);
    assert_eq!(c.run_total_usd, None); // never Some(0.0) — that would read as free
    assert!(!c.cost_measured);
    // Exclusions are still counted and stated without a price.
    assert_eq!(c.excluded_truncated, 4);
    assert!(c.basis_note.contains("no price basis"), "{}", c.basis_note);
}

#[test]
fn zero_successes_yields_null_cost_per_success_not_infinity() {
    let reports = reference();
    let c = summarize(&inputs(&reports, [false; 5]), &cfg());
    assert_eq!(c.cost_per_success_usd, None);
    // The run still cost real money — that figure stays.
    close(c.run_total_usd, 280.0 * PER_SEC);
}

#[test]
fn a_cap_ended_attempt_is_excluded_from_every_figure_and_counted() {
    use AttemptClass::*;
    // Same clean attempt in both runs; the second also burned 900s dying at the cap.
    let clean = vec![task(vec![attempt(10, PassedClean)])];
    let with_cap = vec![task(vec![attempt(10, PassedClean), attempt(900, DiedAtCap)])];

    let a = summarize(&[TaskCostInput { report: &clean[0], meets_pass_k: true }], &cfg());
    let b = summarize(&[TaskCostInput { report: &with_cap[0], meets_pass_k: true }], &cfg());

    assert_eq!(a.cost_per_task_usd, b.cost_per_task_usd, "cap time must not inflate the task cost");
    assert_eq!(a.run_total_usd, b.run_total_usd);
    assert_eq!(a.excluded_truncated, 0);
    assert_eq!(b.excluded_truncated, 1, "the exclusion is counted, never silent");
}

/// A truncated PASS is excluded too: pricing it would quote a cheap number for a
/// run the harness cut short — a pass that shouldn't be sold as clean.
#[test]
fn a_truncated_pass_is_excluded_from_the_attempt_median() {
    use AttemptClass::*;
    let reports = vec![task(vec![attempt(10, PassedClean), attempt(1, TruncatedPass)])];
    let c = summarize(&[TaskCostInput { report: &reports[0], meets_pass_k: false }], &cfg());
    // Median of the ONE clean attempt — not (10+1)/2, which would look cheaper.
    close(c.cost_per_attempt_usd, 10.0 * PER_SEC);
    assert_eq!(c.excluded_truncated, 1);
}

/// "Don't average over what you have": one unmeasured attempt makes the aggregate
/// absent rather than a figure computed from the rest.
#[test]
fn a_missing_wall_clock_makes_the_aggregate_null_not_a_partial_average() {
    use AttemptClass::*;
    let reports = vec![task(vec![
        attempt(10, PassedClean),
        AttemptCost { wall_ms: None, class: PassedClean },
    ])];
    let c = summarize(&[TaskCostInput { report: &reports[0], meets_pass_k: true }], &cfg());
    assert_eq!(c.cost_per_attempt_usd, None);
    assert_eq!(c.cost_per_task_usd, None);
    assert_eq!(c.run_total_usd, None);
    assert!(!c.cost_measured);
}

/// A task whose attempts were ALL excluded has no priceable time — it must not
/// enter the task median as a $0 task, which would drag the figure down.
#[test]
fn a_fully_excluded_task_does_not_enter_the_task_median_as_zero() {
    use AttemptClass::*;
    let reports = vec![
        task(vec![attempt(10, PassedClean)]),
        task(vec![attempt(900, DiedAtCap)]),
    ];
    let c = summarize(&inputs_2(&reports), &cfg());
    close(c.cost_per_task_usd, 10.0 * PER_SEC); // not (10 + 0)/2 = 5
}

fn inputs_2(reports: &[AgenticReport]) -> Vec<TaskCostInput<'_>> {
    reports.iter().map(|report| TaskCostInput { report, meets_pass_k: false }).collect()
}

#[test]
fn basis_note_states_the_price_and_the_upper_bound_caveat() {
    let reports = reference();
    let c = summarize(&inputs(&reports, [true, false, false, false, false]), &cfg());
    assert!(c.basis_note.contains("0.98"), "{}", c.basis_note);
    assert!(c.basis_note.contains("utilization=1.00"), "{}", c.basis_note);
    assert!(c.basis_note.to_lowercase().contains("upper bound"), "{}", c.basis_note);
    assert_eq!(c.basis, "gpu_seconds");
}

/// Utilization scales the rate: half a card at the same hourly price bills the
/// run at twice the per-second rate.
#[test]
fn utilization_scales_the_rate() {
    use AttemptClass::*;
    let reports = vec![task(vec![attempt(10, PassedClean)])];
    let half = CostConfig { utilization: 0.5, ..cfg() };
    let c = summarize(&[TaskCostInput { report: &reports[0], meets_pass_k: true }], &half);
    close(c.cost_per_task_usd, 10.0 * PER_SEC / 0.5);
}

/// A nonsense price is treated as ABSENT, not propagated — a $0/hr rate would
/// otherwise render a real run as free.
#[test]
fn a_non_positive_or_non_finite_price_reads_as_no_basis() {
    let reports = vec![task(vec![AttemptCost { wall_ms: Some(1000), class: AttemptClass::PassedClean }])];
    for bad in [Some(0.0), Some(-1.0), Some(f64::NAN), Some(f64::INFINITY)] {
        let c = summarize(
            &[TaskCostInput { report: &reports[0], meets_pass_k: true }],
            &CostConfig { enabled: true, gpu_hourly_usd: bad, utilization: 1.0 },
        );
        assert!(!c.cost_measured, "price {bad:?} must not produce a figure");
        assert_eq!(c.run_total_usd, None);
    }
    // Same for utilization.
    let c = summarize(
        &[TaskCostInput { report: &reports[0], meets_pass_k: true }],
        &CostConfig { enabled: true, gpu_hourly_usd: Some(PRICE), utilization: 0.0 },
    );
    assert!(!c.cost_measured);
}

#[test]
fn cli_line_prints_na_without_a_price_and_never_zero() {
    let unpriced = summarize(&[], &CostConfig::default());
    let line = cli_line(&unpriced);
    assert!(line.contains("n/a (no price basis)"), "{line}");
    assert!(!line.contains("0.000"), "an unpriced run must never render as $0: {line}");
}

#[test]
fn cli_line_reports_the_two_headline_figures_and_the_exclusions() {
    let reports = reference();
    let c = summarize(&inputs(&reports, [true, false, false, false, false]), &cfg());
    let line = cli_line(&c);
    assert!(line.contains("$/task"), "{line}");
    assert!(line.contains("$/success"), "{line}");
    assert!(line.contains("4 runs excluded"), "{line}");
}
