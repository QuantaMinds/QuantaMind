use super::engine::TaskTally;

/// Closed-form small-sample statistics for the cliff verdict. All pure, no deps —
/// Wilson/Newcombe are ~10-line formulas, and the concentration p-value is exact
/// combinatorics via DP, so the locked stack gains nothing heavier than arithmetic.
///
/// Why not Wald/CLT intervals: at this probe's sample sizes (tens of trials, clustered
/// by task) CLT intervals dramatically underestimate uncertainty and collapse to zero
/// width at 0%/100% — Bowyer, Aitchison & Ivanova (ICML 2025) measure the failure and
/// recommend Wilson-family intervals; Miller (Anthropic 2024) shows position-in-task
/// clustering inflates true SEs 1.1–3×, which is why every consumer of these numbers
/// must also report the TASK count, not just the trial count.

/// z for a 95% interval. Fixed — a configurable confidence level would make two users'
/// verdicts incomparable, the same reason the thinking budget is presets, not a slider.
const Z95: f64 = 1.959_963_985;

/// A two-sided interval on a proportion (or a difference of proportions), in [0,1]
/// (or [-1,1] for a difference).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval {
    pub lo: f64,
    pub hi: f64,
}

/// Wilson score interval for `passed / trials`. `None` when nothing was measured —
/// never a fabricated certainty. At 5/5 this gives ≈[0.57, 1.0], where the naive Wald
/// interval dishonestly collapses to zero width.
pub fn wilson_interval(passed: u32, trials: u32) -> Option<Interval> {
    if trials == 0 {
        return None;
    }
    let n = trials as f64;
    let p = passed as f64 / n;
    let z2 = Z95 * Z95;
    let denom = 1.0 + z2 / n;
    let center = (p + z2 / (2.0 * n)) / denom;
    let half = (Z95 / denom) * ((p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt());
    Some(Interval { lo: (center - half).max(0.0), hi: (center + half).min(1.0) })
}

/// Newcombe's Wilson-based interval (method 10) for the DROP `p_base − p_rung`.
/// The collapse gate: a drop whose interval still includes zero is not a resolvable
/// collapse, however large its point estimate — the sample can't tell it from noise.
pub fn newcombe_drop_interval(base_passed: u32, base_trials: u32, rung_passed: u32, rung_trials: u32) -> Option<Interval> {
    let w1 = wilson_interval(base_passed, base_trials)?;
    let w2 = wilson_interval(rung_passed, rung_trials)?;
    let p1 = base_passed as f64 / base_trials as f64;
    let p2 = rung_passed as f64 / rung_trials as f64;
    let d = p1 - p2;
    let lo = d - ((p1 - w1.lo).powi(2) + (w2.hi - p2).powi(2)).sqrt();
    let hi = d + ((w1.hi - p1).powi(2) + (p2 - w2.lo).powi(2)).sqrt();
    Some(Interval { lo: lo.max(-1.0), hi: hi.min(1.0) })
}

/// Caps past which the exact DP is refused. Far above any real probe (a rung with 100+
/// failures is a broad collapse, not a concentration question) — the guard exists so a
/// pathological input can't grind the verdict layer, and refusal maps to `None`
/// ("not assessed"), never a guessed p.
const MAX_CONC_FAILURES: u32 = 64;
const MAX_CONC_TASKS: usize = 128;

/// Exact p-value that failure placement is task-concentrated: under the null "every
/// trial slot is equally likely to fail" (failures exchangeable across slots), the
/// probability that SOME task holds ≥ the observed maximum per-task failure count.
/// Computed exactly: P = 1 − (#placements with every task < m) / C(N, F), where the
/// numerator is a DP over tasks with per-task caps. For the reviewed run — 4 failures
/// over 5 tasks × 3 slots, 3 in one task — this yields 1 − 1305/1365 ≈ 0.044.
///
/// `None` when there is nothing to assess: fewer than 2 tasks (concentration is
/// undefined), zero failures, or inputs past the DP caps.
pub fn concentration_p_value(by_task: &[TaskTally]) -> Option<f64> {
    if by_task.len() < 2 || by_task.len() > MAX_CONC_TASKS {
        return None;
    }
    let failures: Vec<u32> = by_task.iter().map(|t| t.trials.saturating_sub(t.passed)).collect();
    let total_f: u32 = failures.iter().sum();
    let max_f = *failures.iter().max().unwrap_or(&0);
    if total_f == 0 || total_f > MAX_CONC_FAILURES {
        return None;
    }
    // A max of 1 failure in a task cannot be "concentrated" — p is 1 by definition.
    if max_f <= 1 {
        return Some(1.0);
    }

    let n_total: u32 = by_task.iter().map(|t| t.trials).sum();
    // #ways to place `total_f` failures into per-task slot groups with every task
    // strictly below `max_f`: DP over tasks, a_i ∈ [0, min(n_i, max_f−1)].
    let mut ways_lt: Vec<u128> = vec![0; (total_f + 1) as usize];
    ways_lt[0] = 1;
    for t in by_task {
        let cap = t.trials.min(max_f - 1);
        let mut next = vec![0u128; (total_f + 1) as usize];
        for (f, &w) in ways_lt.iter().enumerate() {
            if w == 0 {
                continue;
            }
            for a in 0..=cap {
                let nf = f as u32 + a;
                if nf > total_f {
                    break;
                }
                next[nf as usize] = next[nf as usize].saturating_add(w.saturating_mul(binom(t.trials, a)));
            }
        }
        ways_lt = next;
    }
    let total_ways = binom(n_total, total_f);
    if total_ways == 0 {
        return None;
    }
    let p = 1.0 - (ways_lt[total_f as usize] as f64 / total_ways as f64);
    Some(p.clamp(0.0, 1.0))
}

/// C(n, k) in u128 — inputs bounded by the concentration caps, so this cannot overflow
/// (C(384, 64) < 2^127 is not reachable: N ≤ MAX_CONC_TASKS × trials-per-task with
/// F ≤ 64; the saturating arithmetic above is belt-and-braces regardless).
fn binom(n: u32, k: u32) -> u128 {
    if k > n {
        return 0;
    }
    let k = k.min(n - k);
    let mut num: u128 = 1;
    for i in 0..k {
        num = num.saturating_mul((n - i) as u128) / (i as u128 + 1);
    }
    num
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tally(id: &str, passed: u32, trials: u32) -> TaskTally {
        TaskTally { task_id: id.into(), passed, trials }
    }

    #[test]
    fn wilson_never_collapses_to_zero_width_at_the_boundaries() {
        // 5/5 — the Wald interval is [1,1] (fabricated certainty); Wilson is honest.
        let w = wilson_interval(5, 5).unwrap();
        assert!(w.lo > 0.55 && w.lo < 0.60, "5/5 lower bound ≈ 0.57: {w:?}");
        assert_eq!(w.hi, 1.0);
        // 0/5 mirrors it.
        let w = wilson_interval(0, 5).unwrap();
        assert_eq!(w.lo, 0.0);
        assert!(w.hi > 0.40 && w.hi < 0.45, "0/5 upper bound ≈ 0.43: {w:?}");
        assert_eq!(wilson_interval(3, 0), None, "nothing measured ⇒ no interval");
    }

    #[test]
    fn newcombe_drop_interval_excludes_zero_only_when_the_sample_can_resolve_it() {
        // The REVIEWED run's real shape: the baseline is single-position (5 trials — one
        // per task), the rung is 15. A 27pp point drop off a 5-trial baseline decisively
        // fails to exclude zero: the sample cannot tell it from noise.
        let d = newcombe_drop_interval(5, 5, 11, 15).unwrap();
        assert!(d.lo < 0.0, "a 5-trial baseline cannot anchor a 27pp drop: {d:?}");
        // The planned scale — 18-task collection (18-trial baseline, 54-trial rungs) —
        // resolves the same proportions.
        let d = newcombe_drop_interval(18, 18, 40, 54).unwrap();
        assert!(d.lo > 0.0, "18 tasks resolve the same drop: {d:?}");
    }

    #[test]
    fn concentration_p_matches_the_hand_computed_reviewed_run() {
        // 5 tasks × 3 slots, 4 failures, 3 concentrated in one task:
        // P(max ≥ 3) = 1 − ways(all tasks ≤ 2)/C(15,4) = 1 − 1305/1365 ≈ 0.04396.
        let by_task = vec![
            tally("a", 3, 3),
            tally("b", 3, 3),
            tally("secret_rotation", 0, 3),
            tally("d", 3, 3),
            tally("e", 2, 3),
        ];
        let p = concentration_p_value(&by_task).unwrap();
        assert!((p - (1.0 - 1305.0 / 1365.0)).abs() < 1e-9, "exact DP vs hand count: {p}");
    }

    #[test]
    fn concentration_refuses_what_it_cannot_assess() {
        // One task: undefined. No failures: nothing to assess. Max 1: p = 1 (never flags).
        assert_eq!(concentration_p_value(&[tally("only", 0, 3)]), None);
        assert_eq!(concentration_p_value(&[tally("a", 3, 3), tally("b", 3, 3)]), None);
        assert_eq!(concentration_p_value(&[tally("a", 2, 3), tally("b", 2, 3)]), Some(1.0));
    }

    #[test]
    fn spread_failures_are_not_concentrated() {
        // 4 failures over 4 different tasks (max 1 per task) — p = 1, no flag possible.
        let by_task: Vec<TaskTally> = (0..5).map(|i| tally(&format!("t{i}"), if i < 4 { 2 } else { 3 }, 3)).collect();
        assert_eq!(concentration_p_value(&by_task), Some(1.0));
    }
}
