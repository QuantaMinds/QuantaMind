use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum AppError {
    #[error("validation: {0}")]
    Validation(String),

    /// Parse ran off the end of the bytes it was given. Distinct from
    /// `Validation` so a partial-buffer reader (e.g. GGUF header peek) can
    /// tell "need more bytes" from "structurally invalid" and fetch more.
    #[error("truncated: {0}")]
    Truncated(String),

    #[error("invalid task schema: {0}")]
    InvalidTaskSchema(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("inference: {0}")]
    Inference(String),

    #[error("io: {0}")]
    Io(String),

    #[error("timeout: {0}")]
    Timeout(String),

    #[error("auth required: {0}")]
    AuthRequired(String),

    #[error("internal: {0}")]
    Internal(String),

    /// A caller-requested cancellation (e.g. Stop Batch) interrupted an in-flight agentic
    /// turn loop. Distinct from every other variant: those are infra/data failures the caller
    /// didn't ask for; this one is expected, user-initiated control flow, and the agentic
    /// runner (`run_agentic_within`) catches it specifically to discard the interrupted run
    /// (never counted as a pass OR a fail) rather than let it fall through to the generic
    /// error path.
    #[error("cancelled: {0}")]
    Cancelled(String),
}

pub type AppResult<T> = Result<T, AppError>;

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::InvalidTaskSchema(e.to_string())
    }
}

impl AppError {
    /// Translate well-known connection failures into actionable copy.
    /// Keeps the original string when there's no friendlier match.
    pub fn friendly(&self) -> String {
        let s = self.to_string();
        let low = s.to_lowercase();
        let looks_like_ollama_down = s.contains("Connection refused")
            || s.contains("error trying to connect")
            || s.contains("os error 61")
            || s.contains("tcp connect error")
            || (s.contains("error sending request") && s.contains("localhost:11434"));
        if looks_like_ollama_down {
            return "Ollama is not running. Start Ollama and try again.".to_string();
        }
        if low.contains("model") && low.contains("not found") {
            return "That model isn't installed. Install it from the Models tab and try again."
                .to_string();
        }
        if low.contains("out of memory") || low.contains("not enough memory") {
            return "Not enough memory to run this model. Try a smaller or more-quantized model."
                .to_string();
        }
        s
    }
}

#[cfg(test)]
#[path = "errors_tests.rs"]
mod tests;
