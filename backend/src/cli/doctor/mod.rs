//! `qm doctor` — the activation fix: one command that tells a stranger on a fresh
//! machine exactly what to do next. Pure of any stdout/process concerns (the `qm`
//! bin renders + maps the exit code); this module only PROBES and REPORTS.
//!
//! Ordered, cheapest-first per backend: reachable? → models? → credential? →
//! native tool-calling? → version. Everything here composes probes that already
//! exist and are already tested (see `commands::remote::remote_health`).

pub mod probe;
pub mod render;
pub mod report;

pub use render::{error_lines, render_human};
pub use report::DoctorReport;

use crate::inference::backend::backend_kind::BackendKind;

/// Every backend, in scan order.
const ALL: [BackendKind; 2] = [
    BackendKind::LlamaCpp,
    BackendKind::VLlm,
];

/// CLI-supplied options for one doctor run.
pub struct DoctorOptions {
    /// Filter to one backend; `None` scans both.
    pub backend: Option<BackendKind>,
    /// `--base` / `QM_BASE` — only honoured when a single backend is targeted (a
    /// base URL is backend-specific, so it's meaningless applied across a scan).
    pub base: Option<String>,
    /// `--model` / `QM_MODEL` — names the model to report on.
    pub model: Option<String>,
    /// Remote bearer credential (resolved from env/keychain, never argv).
    pub api_key: Option<String>,
}

/// Probe the selected backend(s) and return the report. The caller decides how to
/// render it and what to exit with.
pub async fn run_doctor(opts: DoctorOptions) -> DoctorReport {
    let (kinds, base): (Vec<BackendKind>, Option<&str>) = match opts.backend {
        Some(k) => (vec![k], opts.base.as_deref()),
        None => (ALL.to_vec(), None),
    };
    let mut backends = Vec::with_capacity(kinds.len());
    for kind in kinds {
        backends.push(probe::probe_backend(kind, base, opts.model.as_deref(), opts.api_key.as_deref()).await);
    }
    DoctorReport { backends }
}
