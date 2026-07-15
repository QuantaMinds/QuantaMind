//! The **deny-by-default approval gate** — the safety boundary for executing a
//! model's tool call against REAL tools. Nothing side-effecting runs without an
//! explicit approval. This is where the first real model→real-side-effect
//! authority lives, so it is backend-enforced and independent of any UI (P12's
//! modal is just one *source* of the external decision; if it's closed, the
//! backend still denies).
//!
//! Two safety regimes: a **controlled disposable sandbox** is safe by isolation
//! (auto-approve — you cannot gate k automated runs on human clicks); the user's
//! **real tools** are not, so writes always need an explicit Approve, and a
//! server's self-claimed `readOnlyHint` is honored ONLY for a hash-pinned trusted
//! server (annotations are untrusted — tool poisoning can flag a destructive tool
//! read-only).

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Approve,
    Deny,
}

/// A call awaiting a gate decision.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingCall {
    pub server_id: String,
    pub tool: String,
    /// The tool's `readOnlyHint` — only trustworthy when `trusted_server`.
    pub read_only: bool,
    /// True only for a hash-pinned, user-trusted server.
    pub trusted_server: bool,
    pub args: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatePolicy {
    /// Controlled disposable sandbox — safe by isolation; auto-approve all.
    SandboxAutoApprove,
    /// Real tools — auto-approve ONLY a read-only call on a trusted server;
    /// everything else needs an explicit external Approve.
    RealToolsReadOnlyAuto,
    /// Fail-closed: nothing runs without an explicit external Approve.
    DenyByDefault,
}

/// The gate decision. `external` is a human/UI verdict; `None` means no decision
/// yet / timeout / unknown → **Deny** (deny-by-default).
pub fn gate_decision(policy: GatePolicy, call: &PendingCall, external: Option<Decision>) -> Decision {
    match policy {
        GatePolicy::SandboxAutoApprove => Decision::Approve,
        GatePolicy::RealToolsReadOnlyAuto => {
            if call.read_only && call.trusted_server {
                Decision::Approve
            } else {
                external.unwrap_or(Decision::Deny)
            }
        }
        GatePolicy::DenyByDefault => external.unwrap_or(Decision::Deny),
    }
}

#[cfg(test)]
#[path = "gate_tests.rs"]
mod tests;
