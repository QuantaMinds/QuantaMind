//! Grade a system-under-test on the **real end state of a seeded world**, k times.
//!
//! The system under test is not ours. This module seeds a workspace, hands it to an
//! injected *actuator*, and grades what the world looks like afterwards — it never
//! issues a model call and never reads the agent's words. `qm certify` supplies a
//! subprocess actuator (a customer's own agent); tests supply a closure.
//!
//! **Why the actuator is injected.** Everything about *how* the agent is invoked —
//! argv templates, environment policy, timeouts, process groups — is configuration,
//! and configuration belongs at the CLI edge. Keeping it out of here means
//! `inference/` stays free of `std::process`, and every branch of the pass^k and
//! attribution logic is testable with a fake closure and zero subprocesses. It
//! mirrors the shape `mcp::score::score_fs_task` already uses (an injected driver
//! factory), so the two grading paths stay recognisably the same.
//!
//! **Why the world, not the transcript.** A judge reading a transcript grades
//! plausibility; a filesystem read grades validity. An agent that narrates a
//! perfect refund and touches nothing fails here, which is the entire point.

pub mod attempt;
pub mod discriminate;
pub mod grade;
pub mod score;
pub mod workspace;

pub use attempt::{ActuatorReport, ActuatorResult, Attempt, AttemptContext, AttemptStatus};
pub use discriminate::is_discriminating;
pub use score::{run_task, TaskResult};
pub use workspace::Workspace;
