//! Run-artifact persistence, one concern per file (a sub-folder so
//! `persistence/` stays within the folder-taxonomy budget):
//! - `queue`: the resumable batch job log (`app_config_dir/jobs/`) — a leftover
//!   log means an interrupted run; on restart it is loaded (with truncated-tail
//!   healing) and resumed from the next unit.
//! - `transcripts`: per-(model, task) agentic turn-by-turn transcripts
//!   (`app_config_dir/agentic_transcripts/`) for post-mortem of a failing run.
pub mod queue;
pub mod transcripts;
