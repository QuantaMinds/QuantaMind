//! Human rendering + the `[QM-CODE] what — fix` error lines.
//!
//! Stream discipline (enforced by the caller): the rendered report is DATA →
//! stdout; the `[QM-CODE]` fix lines are diagnostics → stderr. So
//! `qm doctor --json | jq` never sees prose. Every fix names the exact command,
//! and never runs it — this audience wants to see what you'd run on their machine.

use super::report::{BackendDoctor, DoctorReport, NativeFc};
use crate::commands::remote::remote_health::RemoteAuthStatus;
use crate::inference::backend::backend_kind::BackendKind;

/// The wire label for a backend (matches `BackendKind`'s serde + `--backend`).
pub fn label(kind: BackendKind) -> &'static str {
    match kind {
        BackendKind::LlamaCpp => "llama_cpp",
        BackendKind::VLlm => "vllm",
        BackendKind::SgLang => "sglang",
    }
}

/// The command that starts this backend (shown, never run).
fn start_hint(kind: BackendKind) -> &'static str {
    match kind {
        BackendKind::LlamaCpp => "llama-server -m <model.gguf> --port 8081",
        BackendKind::VLlm => "vllm serve <model>  (or point --base at a running server)",
        BackendKind::SgLang => "python -m sglang.launch_server --model-path <model> --port 30000",
    }
}

/// The command that gives a reachable-but-empty backend something to run.
fn pull_hint(_kind: BackendKind) -> &'static str {
    "start the server with a model loaded"
}

fn native_fc_str(fc: NativeFc) -> &'static str {
    match fc {
        NativeFc::Supported => "supported",
        NativeFc::Unsupported => "unsupported",
        NativeFc::NotProbed => "not probed",
    }
}

/// One status glyph per backend: ready (runnable), reachable-not-runnable, or down.
fn glyph(b: &BackendDoctor) -> &'static str {
    if b.runnable() {
        "✓ ready"
    } else if b.reachable {
        "! reachable"
    } else {
        "✗ unreachable"
    }
}

/// The full report as human text (→ stdout).
pub fn render_human(report: &DoctorReport) -> String {
    let mut out = String::new();
    for b in &report.backends {
        let mut line = format!("{:<10} {:<28} {}", label(b.kind), b.endpoint, glyph(b));
        if let Some(v) = &b.version {
            line.push_str(&format!("  v{v}"));
        }
        if b.reachable {
            line.push_str(&format!("  models: {}", b.models.len()));
            if b.native_fc != NativeFc::NotProbed {
                line.push_str(&format!("  native-fc: {}", native_fc_str(b.native_fc)));
            }
        }
        if let Some(c) = &b.credential {
            if c.status != RemoteAuthStatus::Ok {
                line.push_str(&format!("  credential: {:?}", c.status));
            }
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.push('\n');
    out.push_str(&next_step(report));
    out.push('\n');
    out
}

/// The single "what do I do now" line at the bottom of the human report.
fn next_step(report: &DoctorReport) -> String {
    let runnable = report.runnable();
    match runnable.as_slice() {
        [] => "No runnable backend — fix the findings above, then re-run `qm doctor`.".into(),
        [one] => {
            let m = one.models.first().map(String::as_str).unwrap_or("<model>");
            format!("Next: qm run --backend {} --model {m}", label(one.kind))
        }
        many => {
            let names: Vec<&str> = many.iter().map(|b| label(b.kind)).collect();
            format!(
                "Ready: {}. Pick one: qm run --backend <{}> --model <model>",
                names.join(", "),
                names.join("|")
            )
        }
    }
}

/// The `[QM-CODE] what — fix` lines (→ stderr). Insecure-key withholding is always
/// surfaced; blocker lines are emitted only when NOTHING is runnable, so a healthy
/// scan (one backend up) doesn't spam a fix line for every backend that's simply off.
pub fn error_lines(report: &DoctorReport) -> Vec<String> {
    let mut out = vec![];
    let none_runnable = report.runnable().is_empty();
    for b in &report.backends {
        if b.insecure_key() {
            out.push(format!(
                "[QM-INSECURE-KEY] {} — a key is set but the URL isn't https; the key was withheld. Use https or drop the key.",
                b.endpoint
            ));
        }
        if none_runnable {
            if let Some(line) = blocker_line(b) {
                out.push(line);
            }
        }
    }
    out
}

/// Why this backend can't run, plus the exact fix. `None` when it IS runnable.
fn blocker_line(b: &BackendDoctor) -> Option<String> {
    if b.runnable() {
        return None;
    }
    let name = label(b.kind);
    // Remote credential problems take precedence over "no models".
    if let Some(c) = &b.credential {
        match c.status {
            RemoteAuthStatus::Unconfigured => {
                return Some(format!("[QM-UNCONFIGURED] {name} has no endpoint — pass --base <url> or set QM_BASE."));
            }
            RemoteAuthStatus::Unauthorized => {
                return Some(format!("[QM-UNAUTHORIZED] {} rejected the API key — check QM_API_KEY.", b.endpoint));
            }
            RemoteAuthStatus::TlsError => {
                return Some(format!("[QM-TLS] {} TLS handshake failed — check the certificate / scheme.", b.endpoint));
            }
            RemoteAuthStatus::NotFound => {
                return Some(format!("[QM-NOT-OPENAI] {} has no /v1/models — check the URL is an OpenAI-compatible server.", b.endpoint));
            }
            RemoteAuthStatus::ServerError => {
                return Some(format!("[QM-SERVER-ERROR] {} returned an error — check the server logs.", b.endpoint));
            }
            RemoteAuthStatus::Unreachable => {} // fall through to the generic unreachable line
            RemoteAuthStatus::Ok => {}          // runnable-gate handled models below
        }
    }
    if !b.reachable {
        return Some(format!(
            "[QM-BACKEND-UNREACHABLE] {name} not reachable at {} — start it: {}",
            b.endpoint,
            start_hint(b.kind)
        ));
    }
    // Reachable, credential fine (or local) — the only thing left is an empty server.
    Some(format!(
        "[QM-NO-MODELS] {name} is up but has no models — pull/serve one: {}",
        pull_hint(b.kind)
    ))
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
