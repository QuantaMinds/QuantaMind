//! `qm init` — zero-config first run. Auto-detect a runnable backend (reusing the
//! doctor scan), record it in `qm.json`, and hand back a ready `QmConfig` the bin
//! writes + runs. Goal: install → real verdict in under two minutes, nothing typed.

use crate::cli::doctor::{run_doctor, DoctorOptions};
use crate::cli::run::config::QmConfig;

/// The starter defaults a fresh run uses.
const DEFAULT_COLLECTION: &str = "easy-coding";
const DEFAULT_PROFILE: &str = "general-agent";

/// Scan every backend and build a config for the first RUNNABLE one (reachable +
/// has a model). `None` when nothing is runnable — the bin then points the user at
/// `qm doctor`, exactly as a fresh machine needs.
pub async fn detect(api_key: Option<String>) -> Option<QmConfig> {
    let report = run_doctor(DoctorOptions { backend: None, base: None, model: None, api_key }).await;
    let first = report.runnable().into_iter().next()?;
    Some(QmConfig {
        // No price is guessed: `--costs` reads n/a until the user declares one.
        costs: None,
        backend: first.kind,
        model: first.models.first().cloned()?,
        collection: DEFAULT_COLLECTION.into(),
        profile: DEFAULT_PROFILE.into(),
        base: None,
    })
}
