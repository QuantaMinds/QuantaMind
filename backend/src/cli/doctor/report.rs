//! Doctor report types + the pure "can you actually run something?" logic.
//!
//! The whole point of `qm doctor` is a stranger's first-run gate: a backend that
//! is reachable but has ZERO models pulled is the single most common trap, so
//! "reachable" is never the success bar — RUNNABLE is (reachable + a model +, for
//! a remote server, a working credential). `exit_code` reads only that, so
//! `qm doctor && qm run` short-circuits honestly.

use crate::commands::remote::remote_health::{RemoteAuthReport, RemoteAuthStatus};
use crate::inference::backend::backend_kind::BackendKind;
use serde::{Deserialize, Serialize};

/// Documented QuantaMind exit-code contract (NOT a Unix convention — see
/// `docs/cli/README.md`). `2` (bad args) is emitted by clap before a report exists.
pub const EXIT_OK: i32 = 0;
pub const EXIT_NO_RUNNABLE: i32 = 3;

/// Native tool-calling probe outcome. Only Ollama exposes a capability endpoint
/// (`/api/show`); every other backend has none yet, so we say `NotProbed` rather
/// than fabricate a result (rule 7 — missing ≠ false).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeFc {
    Supported,
    Unsupported,
    NotProbed,
}

/// One backend's full diagnostic. `endpoint` is already redacted (scheme+host+port
/// only — rule 7f). `credential` is `Some` only for the remote backends.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BackendDoctor {
    pub kind: BackendKind,
    pub endpoint: String,
    pub reachable: bool,
    pub version: Option<String>,
    pub models: Vec<String>,
    pub credential: Option<RemoteAuthReport>,
    pub native_fc: NativeFc,
}

impl BackendDoctor {
    pub fn is_remote(&self) -> bool {
        matches!(self.kind, BackendKind::VLlm | BackendKind::SgLang)
    }

    /// Can this backend run a model right now? Reachable AND has ≥1 model AND, for a
    /// remote server, the credential resolved `Ok`. A reachable-but-empty server is
    /// deliberately NOT runnable.
    pub fn runnable(&self) -> bool {
        if !self.reachable || self.models.is_empty() {
            return false;
        }
        if self.is_remote() {
            return matches!(self.credential.as_ref().map(|c| c.status), Some(RemoteAuthStatus::Ok));
        }
        true
    }

    /// A key was configured but withheld because the URL isn't https/loopback.
    pub fn insecure_key(&self) -> bool {
        self.credential.as_ref().is_some_and(|c| c.insecure_key)
    }
}

/// The whole diagnostic — one entry per probed backend (1 when `--backend` targets
/// one, up to 5 in scan mode).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DoctorReport {
    pub backends: Vec<BackendDoctor>,
}

impl DoctorReport {
    pub fn runnable(&self) -> Vec<&BackendDoctor> {
        self.backends.iter().filter(|b| b.runnable()).collect()
    }

    /// `0` iff at least one probed backend is runnable, else `3`. "A report was
    /// produced" is output, not success — success is "you can run something".
    pub fn exit_code(&self) -> i32 {
        if self.runnable().is_empty() {
            EXIT_NO_RUNNABLE
        } else {
            EXIT_OK
        }
    }
}

#[cfg(test)]
#[path = "report_tests.rs"]
mod tests;
