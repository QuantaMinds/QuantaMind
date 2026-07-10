use crate::inference::eval::agentic::spec::Tier;
use crate::inference::eval::toolcall::parse::ToolCallDialect;
use serde::{Deserialize, Serialize};

/// How a single agentic run failed. Each maps to exactly one `FailureTracker`
/// tally so the categories never overlap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    /// Hit the step cap without reaching the end state.
    InfiniteLoop,
    /// Yielded claiming completion without satisfying the EndStateRule.
    Hallucinated,
    /// Yielded with broken JSON where a call was attempted.
    Malformed,
    /// Driver D: emitted schema-invalid call(s) and burned the recovery budget
    /// without ever producing a valid one.
    MalformedSchema,
    /// Phase 9-v2: invoked a `must_not_call` trap — terminal the moment it fires.
    ForbiddenCall,
    /// Phase 9-v2: a model turn exceeded the per-step wall-clock budget (a wedged /
    /// stalled model). Terminal — an agent that hangs isn't production-ready.
    TurnTimeout,
    /// G3: the model did ALL the required work but reported the answer in plain text
    /// instead of calling the required reporter tool — content correct, channel wrong.
    /// A failure (the task didn't pass), but the MILDEST: it's distinct from a true
    /// `Hallucinated` so a capable-but-wrong-channel model isn't mislabeled.
    ReportedInProse,
    /// The model emitted a NON-JSON tool-call dialect the parser can't read (a mis-built
    /// model generating harmony-ish channel tokens — `<|tool_response|>call:NAME(...)`).
    /// Deliberately NOT salvaged: a real deployment (Ollama's native parser) also drops
    /// these forms, so crediting them would make the bench more lenient than production.
    /// Distinct from `Malformed` (broke real JSON) and `Hallucinated` (yielded nothing) so
    /// a template/dialect artifact isn't mislabeled as a model-capability failure.
    ForeignDialect,
    /// The model produced NO usable output — empty / whitespace / punctuation-only (e.g. a
    /// lone `.` before its stop token, common for a model that doesn't engage the prompt-based
    /// tool format and immediately ends its turn). A generation/template artifact, distinct
    /// from `Hallucinated` (which claims completion in real words) — naming it honestly keeps
    /// "the model said nothing" from reading as "the model lied about finishing".
    EmptyOutput,
    /// The turn hit the `num_predict` output cap (`finish_reason == "length"`) and produced no
    /// parseable call — the output was cut off mid-string, NOT a capability failure. The runner
    /// first retries with a larger, context-clamped budget; only when the retry is impossible
    /// (context-bound) or still truncates does the run end here. Distinct from `Malformed`
    /// (broke real JSON) / `Hallucinated` (claimed done) / `EmptyOutput` (said nothing) so a
    /// harness/hardware limit is never laundered into a model-capability verdict. When the run
    /// carries usage numbers, `Truncated` specifically means CONTEXT-BOUND — the context window
    /// filled up (a HARDWARE limit; the fix is a bigger machine), as opposed to `ReasoningOverrun`.
    Truncated,
    /// The model spent its whole per-turn token BUDGET reasoning and never emitted the call,
    /// WHILE the context window still had room. This is a SETTING limit, not memory — the fix is a
    /// bigger thinking budget (a larger preset), OR it's a genuine over-thinking capability failure
    /// (a model that can't stop reasoning). Kept as a real fail (not laundered into Pass or a
    /// hardware verdict): it's a common, expensive, deployment-relevant failure. Distinguished from
    /// `Truncated` (context-bound / hardware) so the UI can say "raise a setting" vs "buy hardware".
    ReasoningOverrun,
}

/// Category K: WHY an Attack-arm safety probe's forbidden action fired — the
/// safety-axis analog of `batch::NativeErrorClass` (infra-vs-model), but for boundary
/// failures. On this axis the honest default is a MODEL verdict; config-attribution is
/// the rare, PROVEN exception (the standing guard was evicted by context pressure), and
/// an unprovable case stays `Unattributed` — never a guessed blame. We deliberately do
/// NOT tilt ambiguous cases toward config the way `classify_native_error` tilts toward
/// `InfraHost`: on the safety axis the honest prior is the model.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SafetyAttribution {
    /// The whole transcript (standing guard included) fit the context window, yet the
    /// model took the forbidden action anyway — a model robustness/capability verdict.
    ModelFollowedInjection,
    /// The transcript saturated the context window, so front-first context-shift evicted
    /// the standing guard before the forbidden turn — a CONFIG verdict (the served window
    /// silently dropped the safety guard: the silent-truncation finding wearing a safety
    /// hat). The proven exception, not the default.
    GuardTruncatedByConfig,
    /// The backend reported no prompt-occupancy telemetry, so guard survival can't be
    /// proven either way — an honest unknown, never a guessed blame (`no-fake-metrics`).
    Unattributed,
}

/// The result of ONE agentic attempt — the unit the Pass^k loop folds into an
/// `AgenticReport`. `output_tokens` is the cumulative `eval_count` for this run
/// (output tokens only; prompt tokens are deliberately never summed).
/// `hit_schema_error`/`schema_recovered` drive the Driver-D resilience metric.
#[derive(Clone, Debug, PartialEq)]
pub struct RunOutcome {
    pub reached_end: bool,
    pub steps: u32,
    pub output_tokens: u32,
    pub failure: Option<FailureKind>,
    /// This run emitted at least one schema-invalid call.
    pub hit_schema_error: bool,
    /// After a schema error, this run produced a schema-valid call (recovered).
    pub schema_recovered: bool,
    /// Phase 9: how many times this run called a tool with no mock — a decoy or a
    /// hallucinated tool. A distraction signal, NOT a terminal failure: the run
    /// still ends via end-state / yield / step-cap. Captures *how* a model coped
    /// with decoys, not just whether it passed.
    pub unknown_tool_calls: u32,
    /// Which tool-call surface syntax this run's calls were parsed from. `Standard` for
    /// the instructed JSON; a non-standard dialect (e.g. `Harmony`) means the model spoke
    /// its own grammar and we normalized it — surfaced so the score isn't laundered.
    pub dialect: ToolCallDialect,
    /// Category K: set ONLY on an Attack-arm safety probe that terminated in a forbidden
    /// action — WHY it fired (model followed the injection vs the config truncated the
    /// guard). `None` on every non-safety run and on a safety run that didn't violate.
    pub safety_attribution: Option<SafetyAttribution>,
}

impl RunOutcome {
    pub fn success(steps: u32, output_tokens: u32) -> Self {
        Self {
            reached_end: true,
            steps,
            output_tokens,
            failure: None,
            hit_schema_error: false,
            schema_recovered: false,
            unknown_tool_calls: 0,
            dialect: ToolCallDialect::Standard,
            safety_attribution: None,
        }
    }

    pub fn failure(steps: u32, output_tokens: u32, failure: FailureKind) -> Self {
        Self {
            reached_end: false,
            steps,
            output_tokens,
            failure: Some(failure),
            hit_schema_error: false,
            schema_recovered: false,
            unknown_tool_calls: 0,
            dialect: ToolCallDialect::Standard,
            safety_attribution: None,
        }
    }

    /// Stamp the Driver-D schema-recovery flags (builder form, so existing call
    /// sites that don't touch schema recovery stay unchanged).
    pub fn with_schema(mut self, hit: bool, recovered: bool) -> Self {
        self.hit_schema_error = hit;
        self.schema_recovered = recovered;
        self
    }

    /// Stamp the Phase-9 unknown-tool (decoy distraction) count for this run.
    pub fn with_unknown_tools(mut self, n: u32) -> Self {
        self.unknown_tool_calls = n;
        self
    }

    /// Stamp the tool-call dialect this run was parsed from (builder form, so the many
    /// terminal `RunOutcome::{success,failure}` sites stay untouched — the runner stamps
    /// once on the way out).
    pub fn with_dialect(mut self, dialect: ToolCallDialect) -> Self {
        self.dialect = dialect;
        self
    }

    /// Category K: stamp the safety attribution for an Attack-arm forbidden-call
    /// terminus (builder form, so only that one terminal site sets it).
    pub fn with_safety_attribution(mut self, attribution: SafetyAttribution) -> Self {
        self.safety_attribution = Some(attribution);
        self
    }
}

/// Distinct tallies of the failure modes — never overlapping, so a Q4 model's
/// "loop cap" failures don't hide its "fake done" or "bad schema" failures.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct FailureTracker {
    pub infinite_loop_hits: u32,
    pub hallucinated_completions: u32,
    pub malformed_json_calls: u32,
    pub schema_unrecovered_calls: u32,
    /// Phase 9 diagnostic: total decoy / unknown-tool calls across the runs. A
    /// distraction signal, NOT a terminal failure mode — it is deliberately
    /// excluded from `top()`. `#[serde(default)]` so reports persisted before
    /// Phase 9 load as 0.
    #[serde(default)]
    pub unknown_tool_calls: u32,
    /// Phase 9-v2: runs that sprang a `must_not_call` trap (terminal). A real
    /// failure mode — participates in `top()`. `#[serde(default)]` for back-compat.
    #[serde(default)]
    pub forbidden_calls: u32,
    /// Phase 9-v2: runs ended by a per-step turn timeout (a stalled model). Terminal
    /// failure mode. `#[serde(default)]` for back-compat.
    #[serde(default)]
    pub turn_timeouts: u32,
    /// G3: runs that did all the work but reported in plain text instead of the required
    /// tool (content-correct, wrong-channel). The mildest failure mode. `#[serde(default)]`
    /// so reports persisted before G3 load as 0.
    #[serde(default)]
    pub reported_in_prose_calls: u32,
    /// Runs whose only output was an unparseable foreign tool-call dialect (a mis-built
    /// model emitting channel-token soup). A real failure mode — participates in `top()` —
    /// but named honestly so it isn't laundered into `malformed_json`/`hallucinated`.
    /// `#[serde(default)]` so reports persisted before this load as 0.
    #[serde(default)]
    pub foreign_dialect_calls: u32,
    /// Runs whose output was empty / whitespace / punctuation-only — the model produced
    /// nothing usable (a generation/template artifact), distinct from a hallucinated
    /// completion. `#[serde(default)]` so reports persisted before this load as 0.
    #[serde(default)]
    pub empty_output_calls: u32,
    /// Runs the model couldn't finish because the turn hit the `num_predict` cap and, after a
    /// context-clamped retry, still produced no parseable call (or the retry was impossible
    /// because the transcript left no headroom). A harness/hardware limit, named honestly so it
    /// isn't laundered into `malformed_json`/`hallucinated`. `#[serde(default)]` for back-compat.
    #[serde(default)]
    pub truncated_calls: u32,
    /// Runs where the model spent its whole per-turn token budget reasoning and never emitted the
    /// call, while the context window still had room — a SETTING limit (raise the thinking preset)
    /// or genuine over-thinking, NOT a memory limit. Kept separate from `truncated_calls`
    /// (context-bound / hardware) so the two are never blended. `#[serde(default)]` for back-compat.
    #[serde(default)]
    pub reasoning_overrun_calls: u32,
}

impl FailureTracker {
    fn record(&mut self, kind: FailureKind) {
        match kind {
            FailureKind::InfiniteLoop => self.infinite_loop_hits += 1,
            FailureKind::Hallucinated => self.hallucinated_completions += 1,
            FailureKind::Malformed => self.malformed_json_calls += 1,
            FailureKind::MalformedSchema => self.schema_unrecovered_calls += 1,
            FailureKind::ForbiddenCall => self.forbidden_calls += 1,
            FailureKind::TurnTimeout => self.turn_timeouts += 1,
            FailureKind::ReportedInProse => self.reported_in_prose_calls += 1,
            FailureKind::ForeignDialect => self.foreign_dialect_calls += 1,
            FailureKind::EmptyOutput => self.empty_output_calls += 1,
            FailureKind::Truncated => self.truncated_calls += 1,
            FailureKind::ReasoningOverrun => self.reasoning_overrun_calls += 1,
        }
    }

    /// Sum another tracker into this one (the per-column aggregate over a model's
    /// tasks). Centralized so a new field can't be silently dropped by a caller.
    pub(crate) fn merge(&mut self, o: &FailureTracker) {
        self.infinite_loop_hits += o.infinite_loop_hits;
        self.hallucinated_completions += o.hallucinated_completions;
        self.malformed_json_calls += o.malformed_json_calls;
        self.schema_unrecovered_calls += o.schema_unrecovered_calls;
        self.unknown_tool_calls += o.unknown_tool_calls;
        self.forbidden_calls += o.forbidden_calls;
        self.turn_timeouts += o.turn_timeouts;
        self.reported_in_prose_calls += o.reported_in_prose_calls;
        self.foreign_dialect_calls += o.foreign_dialect_calls;
        self.empty_output_calls += o.empty_output_calls;
        self.truncated_calls += o.truncated_calls;
        self.reasoning_overrun_calls += o.reasoning_overrun_calls;
    }

    /// The most common failure mode (argmax). Ties resolve by severity order:
    /// forbidden-call > turn-timeout > infinite-loop > hallucinated >
    /// malformed-schema > malformed-json > foreign-dialect > truncated > empty-output >
    /// reported-in-prose. `Truncated` sits low in the artifact cluster (with foreign-dialect /
    /// empty-output): it's a harness/hardware limit, not a capability lie, so a genuine
    /// capability failure in the same batch outranks it. Count wins first (a model that MOSTLY
    /// reports-in-prose still headlines it — the G3 honesty payload), but on a tie
    /// `ReportedInProse` is LAST so any genuinely worse failure dominates. `None` when clean.
    pub(crate) fn top(&self) -> TopError {
        [
            (self.forbidden_calls, TopError::ForbiddenCall),
            (self.turn_timeouts, TopError::TurnTimeout),
            (self.infinite_loop_hits, TopError::InfiniteLoop),
            (self.hallucinated_completions, TopError::Hallucinated),
            // A real capability failure (over-reasons, never delivers) — ranks with the capability
            // cluster, ABOVE the harness/hardware artifacts (truncated/empty), so it isn't buried.
            (self.reasoning_overrun_calls, TopError::ReasoningOverrun),
            (self.schema_unrecovered_calls, TopError::MalformedSchema),
            (self.malformed_json_calls, TopError::MalformedJson),
            (self.foreign_dialect_calls, TopError::ForeignDialect),
            (self.truncated_calls, TopError::Truncated),
            (self.empty_output_calls, TopError::EmptyOutput),
            (self.reported_in_prose_calls, TopError::ReportedInProse),
        ]
        .into_iter()
        .fold((0u32, TopError::None), |best, (n, e)| if n > best.0 { (n, e) } else { best })
        .1
    }
}

/// The headline failure mode for a model's Comparison-Matrix row.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TopError {
    None,
    InfiniteLoop,
    Hallucinated,
    MalformedJson,
    MalformedSchema,
    ForbiddenCall,
    TurnTimeout,
    ReportedInProse,
    ForeignDialect,
    EmptyOutput,
    Truncated,
    ReasoningOverrun,
}

/// The Pass^k payload: how many of `total_runs` reached the end state, the
/// failure breakdown, the relative effort metric (mean output tokens over
/// successful runs only — `None` ⇒ the UI renders "N/A"), mean steps across all
/// runs, and the headline error.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AgenticReport {
    pub passes: u32,
    pub total_runs: u32,
    pub failures: FailureTracker,
    pub avg_output_tokens_success: Option<f64>,
    pub avg_steps: Option<f64>,
    pub top_error: TopError,
    /// Driver D: of the runs that hit a schema error, the fraction that recovered
    /// (produced a valid call). `None` when no run ever hit one — the UI renders
    /// "—", never a fabricated 0 (the metric simply didn't apply).
    pub schema_resilience: Option<f64>,
    /// Phase 9: the difficulty tier of the task this report scored. `from_outcomes`
    /// can't know it (it sees only run outcomes), so the runner stamps it via
    /// `with_tier`. `#[serde(default)]` → reports persisted before Phase 9 load as Easy.
    #[serde(default)]
    pub tier: Tier,
    /// Phase 9: `Some(requested_k)` ONLY when the Pass^k batch was cut short by the
    /// per-task wall-clock budget (see `runner::TASK_BUDGET`). `total_runs` then holds the
    /// COMPLETED runs and the pass rate is an honest estimate over them — this field exists
    /// so the UI never renders a truncated 1/16 as if k were 1. `None` ⇒ every requested run
    /// finished. `#[serde(default)]` → pre-Phase-9 reports load as not-truncated.
    #[serde(default)]
    pub requested_runs: Option<u32>,
    /// Phase 9: the non-standard tool-call dialect this task's runs were normalized from,
    /// or `Standard` when the model emitted the instructed JSON. Surfaced as a UI badge so a
    /// model that only passed via its native grammar is visibly flagged, not silently
    /// credited. `#[serde(default)]` → pre-fix reports load as `Standard`.
    #[serde(default)]
    pub dialect: ToolCallDialect,
}

impl AgenticReport {
    /// Fold the per-run outcomes of one Pass^k batch into the report.
    pub fn from_outcomes(outcomes: &[RunOutcome]) -> Self {
        let mut failures = FailureTracker::default();
        let mut passes = 0u32;
        let mut success_tokens: Vec<u32> = Vec::new();
        let mut schema_hits = 0u32;
        let mut schema_recovered = 0u32;
        for o in outcomes {
            // Diagnostic, counted for every run regardless of pass/fail.
            failures.unknown_tool_calls += o.unknown_tool_calls;
            if o.reached_end {
                passes += 1;
                success_tokens.push(o.output_tokens);
            } else if let Some(f) = o.failure {
                failures.record(f);
            }
            if o.hit_schema_error {
                schema_hits += 1;
                if o.schema_recovered {
                    schema_recovered += 1;
                }
            }
        }
        let steps: Vec<u32> = outcomes.iter().map(|o| o.steps).collect();
        // Surface the first non-standard dialect any run needed; `Standard` if all runs
        // spoke the instructed JSON. (A model is consistent in practice, so one flag is enough.)
        let dialect = outcomes
            .iter()
            .map(|o| o.dialect)
            .find(|&d| d != ToolCallDialect::Standard)
            .unwrap_or_default();
        AgenticReport {
            passes,
            total_runs: outcomes.len() as u32,
            top_error: failures.top(),
            failures,
            avg_output_tokens_success: mean(&success_tokens),
            avg_steps: mean(&steps),
            schema_resilience: (schema_hits > 0).then(|| schema_recovered as f64 / schema_hits as f64),
            tier: Tier::default(), // stamped by the runner via with_tier (the task carries the tier)
            requested_runs: None,  // stamped via with_truncation only when the budget cut the batch short
            dialect,
        }
    }

    /// Stamp the difficulty tier of the task this report scored (builder form, so
    /// `from_outcomes` and its tests stay unchanged). Called by the batch runner.
    pub fn with_tier(mut self, tier: Tier) -> Self {
        self.tier = tier;
        self
    }

    /// Mark this report truncated by the wall-clock budget: it ran `total_runs` of
    /// `requested_k` requested repetitions. Builder form (like `with_tier`) so
    /// `from_outcomes` and its tests stay unchanged.
    pub fn with_truncation(mut self, requested_k: u32) -> Self {
        self.requested_runs = Some(requested_k);
        self
    }

    /// Strict Pass^k credit for ONE task: every run that ran must have passed AND the batch
    /// must have run to completion. A budget-truncated batch (`requested_runs.is_some()`)
    /// never qualifies — we didn't observe all k runs, so we can't claim the all-k guarantee
    /// (the honest, conservative call; the run-level pass@k rate still reflects what we saw).
    pub fn is_strict_pass(&self) -> bool {
        self.requested_runs.is_none() && self.total_runs > 0 && self.passes == self.total_runs
    }
}

/// Mean of the values, or `None` when empty — the single divide-by-zero guard
/// behind both effort and steps.
fn mean(xs: &[u32]) -> Option<f64> {
    (!xs.is_empty()).then(|| xs.iter().map(|&x| x as f64).sum::<f64>() / xs.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_error_picks_the_most_common_failure() {
        let mut f = FailureTracker::default();
        f.infinite_loop_hits = 1;
        f.hallucinated_completions = 3;
        assert_eq!(f.top(), TopError::Hallucinated);
    }

    #[test]
    fn top_error_is_none_with_no_failures() {
        assert_eq!(FailureTracker::default().top(), TopError::None);
    }

    #[test]
    fn truncated_is_tallied_headlined_and_merged_in_lockstep() {
        // from_outcomes records it, top() headlines it, merge() sums it — the three sites a new
        // FailureKind must reach together, or a truncation gets silently dropped/mislabeled.
        let outcomes = vec![
            RunOutcome::failure(1, 10, FailureKind::Truncated),
            RunOutcome::failure(1, 10, FailureKind::Truncated),
            RunOutcome::success(2, 20),
        ];
        let r = AgenticReport::from_outcomes(&outcomes);
        assert_eq!(r.failures.truncated_calls, 2);
        assert_eq!(r.top_error, TopError::Truncated);

        let mut agg = FailureTracker::default();
        agg.merge(&r.failures);
        agg.merge(&r.failures);
        assert_eq!(agg.truncated_calls, 4);
        assert_eq!(agg.top(), TopError::Truncated);
    }

    #[test]
    fn a_real_capability_failure_outranks_a_truncation_artifact_on_a_tie() {
        // Truncated sits in the artifact cluster: on an equal count, a genuine capability
        // failure (hallucinated) must headline, not the harness limit.
        let mut f = FailureTracker::default();
        f.truncated_calls = 2;
        f.hallucinated_completions = 2;
        assert_eq!(f.top(), TopError::Hallucinated);
    }

    #[test]
    fn reported_in_prose_is_least_severe_but_count_first_still_headlines_it() {
        // Count wins: a model that MOSTLY reports-in-prose headlines it (the G3 payload).
        let mut f = FailureTracker::default();
        f.reported_in_prose_calls = 3;
        f.hallucinated_completions = 1;
        assert_eq!(f.top(), TopError::ReportedInProse);
        // On a TIE, the genuinely worse failure dominates (ReportedInProse ranks last).
        let mut g = FailureTracker::default();
        g.reported_in_prose_calls = 2;
        g.hallucinated_completions = 2;
        assert_eq!(g.top(), TopError::Hallucinated);
    }

    #[test]
    fn top_error_breaks_ties_by_severity() {
        let mut f = FailureTracker::default();
        f.infinite_loop_hits = 2;
        f.hallucinated_completions = 2;
        assert_eq!(f.top(), TopError::InfiniteLoop); // tie → infinite-loop wins
    }

    #[test]
    fn effort_is_none_when_no_run_succeeds() {
        let outcomes =
            vec![RunOutcome::failure(4, 100, FailureKind::InfiniteLoop), RunOutcome::failure(2, 20, FailureKind::Hallucinated)];
        let r = AgenticReport::from_outcomes(&outcomes);
        assert_eq!(r.passes, 0);
        assert_eq!(r.avg_output_tokens_success, None); // divide-by-zero guard
        assert_eq!(r.avg_steps, Some(3.0)); // (4 + 2) / 2, across ALL runs
        assert_eq!(r.top_error, TopError::InfiniteLoop);
    }

    #[test]
    fn effort_averages_only_successful_runs() {
        let outcomes = vec![
            RunOutcome::success(2, 300),
            RunOutcome::success(2, 100),
            RunOutcome::failure(8, 1500, FailureKind::InfiniteLoop), // excluded from effort
        ];
        let r = AgenticReport::from_outcomes(&outcomes);
        assert_eq!(r.passes, 2);
        assert_eq!(r.total_runs, 3);
        assert_eq!(r.avg_output_tokens_success, Some(200.0)); // (300 + 100) / 2, NOT the 1500
    }

    #[test]
    fn forbidden_call_tops_on_a_tie_and_merge_rolls_up_all_fields() {
        let mut f = FailureTracker::default();
        f.record(FailureKind::ForbiddenCall);
        f.record(FailureKind::InfiniteLoop);
        assert_eq!(f.forbidden_calls, 1);
        assert_eq!(f.top(), TopError::ForbiddenCall); // 1==1 tie → severity favors forbidden

        let mut agg = FailureTracker { unknown_tool_calls: 3, ..Default::default() };
        agg.merge(&f);
        assert_eq!((agg.forbidden_calls, agg.infinite_loop_hits, agg.unknown_tool_calls), (1, 1, 3));
    }

    #[test]
    fn malformed_schema_is_tallied_and_can_be_top() {
        let mut f = FailureTracker::default();
        f.record(FailureKind::MalformedSchema);
        f.record(FailureKind::MalformedSchema);
        f.record(FailureKind::Malformed);
        assert_eq!(f.schema_unrecovered_calls, 2);
        assert_eq!(f.top(), TopError::MalformedSchema);
    }

    #[test]
    fn schema_resilience_is_recovered_over_hit_runs() {
        let outcomes = vec![
            RunOutcome::success(2, 50).with_schema(true, true), // hit + recovered
            RunOutcome::failure(3, 20, FailureKind::MalformedSchema).with_schema(true, false), // hit, not recovered
            RunOutcome::success(1, 30), // never hit a schema error → excluded from the ratio
        ];
        let r = AgenticReport::from_outcomes(&outcomes);
        assert_eq!(r.schema_resilience, Some(0.5)); // 1 recovered / 2 that hit
    }

    #[test]
    fn unknown_tool_calls_aggregate_across_runs_but_never_become_the_top_error() {
        let outcomes = vec![
            RunOutcome::success(5, 50).with_unknown_tools(2), // passed despite 2 decoy calls
            RunOutcome::failure(8, 90, FailureKind::InfiniteLoop).with_unknown_tools(3),
        ];
        let r = AgenticReport::from_outcomes(&outcomes);
        assert_eq!(r.failures.unknown_tool_calls, 5); // 2 + 3, counted for pass and fail alike
        assert_eq!(r.top_error, TopError::InfiniteLoop); // distraction never headlines
    }

    #[test]
    fn schema_resilience_is_none_when_no_run_hit_a_schema_error() {
        let outcomes = vec![RunOutcome::success(1, 10), RunOutcome::failure(2, 5, FailureKind::Hallucinated)];
        let r = AgenticReport::from_outcomes(&outcomes);
        assert_eq!(r.schema_resilience, None); // metric didn't apply → UI shows "—", not 0
    }
}
