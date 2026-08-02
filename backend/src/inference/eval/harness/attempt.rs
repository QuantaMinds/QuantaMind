use std::path::Path;

/// What the harness handed the actuator for one attempt. Pure data: the actuator
/// is whatever the caller injects, so nothing here knows about argv, processes, or
/// environment variables — that is the CLI layer's business.
pub struct AttemptContext<'a> {
    pub task_id: &'a str,
    /// The task text, verbatim as authored. No preamble is added: the agent under
    /// test is not ours to instruct about our tooling.
    pub goal: &'a str,
    /// The graded root. Everything the oracle reads lives under here.
    pub workspace: &'a Path,
    /// A JSON description of the task, written OUTSIDE the graded root so the
    /// end state is exactly the seed plus what the agent did — never our own files.
    pub task_file: &'a Path,
    /// Where a trace exporter should write, also outside the graded root.
    pub otel_dir: &'a Path,
    /// The sqlite file, for db worlds only.
    pub db: Option<&'a Path>,
    /// 1-based attempt number within pass^k.
    pub attempt: u32,
}

/// How the agent's run ended, as observed from outside. Deliberately narrow — the
/// harness is blind to the agent's internals and must not invent a cause it
/// cannot see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActuatorResult {
    Exited { code: i32 },
    TimedOut,
    SpawnFailed { reason: String },
}

/// One actuator run: how it ended, how long it took, and whatever it said on
/// stderr (already redacted by the caller).
#[derive(Debug, Clone)]
pub struct ActuatorReport {
    pub result: ActuatorResult,
    pub wall_ms: u64,
    pub stderr_tail: Vec<String>,
}

/// The outcome of one attempt. Seven mutually exclusive states — an attempt is
/// never in two, and the label always names the *real* cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptStatus {
    /// Exit 0 and the oracle passed.
    Passed,
    /// Exit 0 but the world is wrong. Carries the oracle's own failure strings —
    /// never a summary, so the user sees exactly which assertion failed.
    FailedState { failures: Vec<String> },
    /// Non-zero exit.
    ///
    /// This counts as a pass^k **failure**, not "inconclusive". The exit could be a
    /// 429 from the agent's own provider — but we are deliberately blind to the
    /// agent's internals and therefore cannot tell "provider flake" from "your
    /// agent doesn't retry 429s". Calling it inconclusive would be inventing a
    /// cause we cannot observe. The code and stderr tail are printed so the user
    /// can attribute it themselves.
    AgentExitNonZero { code: i32 },
    /// Wall-clock cap hit; the process group was killed.
    ///
    /// A failure even when the oracle would have passed: an agent that completes
    /// the work and then hangs is not deployable. `oracle_would_have_passed`
    /// records which of the two it was, so the report names the honest cause
    /// ("hung after finishing") instead of implying the work was wrong.
    AgentTimeout { limit_ms: u64, oracle_would_have_passed: bool },
    /// The command could not be started at all. Not the agent's fault and not a
    /// verdict — inconclusive.
    AgentSpawnFailed { reason: String },
    /// Our own machinery failed: seeding, sqlite, the scratch dir. Inconclusive;
    /// a broken harness must never read as a bad agent.
    HarnessError { reason: String },
}

impl AttemptStatus {
    /// Does this attempt count against strict pass^k?
    ///
    /// Inconclusive states (spawn failure, harness error) are NOT failures — but
    /// they are not passes either, so a run containing them can never be green.
    /// The caller enforces that separately; see `TaskResult::inconclusive`.
    pub fn is_pass(&self) -> bool {
        matches!(self, AttemptStatus::Passed)
    }

    /// True when we could not measure this attempt at all.
    pub fn is_inconclusive(&self) -> bool {
        matches!(self, AttemptStatus::AgentSpawnFailed { .. } | AttemptStatus::HarnessError { .. })
    }

    /// A short, honest label. Never says "failed" for something we didn't measure.
    pub fn label(&self) -> String {
        match self {
            AttemptStatus::Passed => "PASSED".into(),
            AttemptStatus::FailedState { failures } => {
                format!("FAILED STATE — {}", failures.join("; "))
            }
            AttemptStatus::AgentExitNonZero { code } => format!("AGENT EXITED {code}"),
            AttemptStatus::AgentTimeout { limit_ms, oracle_would_have_passed } => {
                let secs = *limit_ms as f64 / 1000.0;
                if *oracle_would_have_passed {
                    format!(
                        "AGENT TIMED OUT after {secs:.0}s (process group killed) — \
                         the world was correct, the agent hung after finishing"
                    )
                } else {
                    format!("AGENT TIMED OUT after {secs:.0}s (process group killed)")
                }
            }
            AttemptStatus::AgentSpawnFailed { reason } => format!("COULD NOT START AGENT — {reason}"),
            AttemptStatus::HarnessError { reason } => format!("HARNESS ERROR — {reason}"),
        }
    }
}

/// One attempt's full record.
#[derive(Debug, Clone)]
pub struct Attempt {
    /// 1-based.
    pub n: u32,
    pub status: AttemptStatus,
    /// Measured wall-clock for the actuator. `None` when nothing ran — never 0,
    /// which would read as "instant".
    pub wall_ms: Option<u64>,
    /// `None` on timeout and on spawn failure: there was no exit code to observe.
    /// Never `0` and never `-1`, both of which would be fabrications.
    pub exit_code: Option<i32>,
    pub stderr_tail: Vec<String>,
}
