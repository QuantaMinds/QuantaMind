//! Everything behind the headless `qm` CLI, in one place (the bin at
//! `src/bin/qm/main.rs` stays a thin arg-parser over these engines).
//!
//! - [`doctor`] — diagnose backends (per-engine probing lives in `doctor::engines`).
//! - [`run`] — run the built-in suite → a readiness verdict.
//! - [`init`] — auto-detect a backend, write `qm.json`, run.
//!
//! These are the command *engines* (pure of stdout/process concerns); they are NOT
//! `#[tauri::command]`s. The GUI keeps its own commands under `commands/`.

pub mod cliff;
pub mod doctor;
pub mod init;
pub mod run;
