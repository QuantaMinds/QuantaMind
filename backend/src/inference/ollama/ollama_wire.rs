use crate::inference::generate::generate_options::GenerateOptions;
use crate::inference::generate::generate_stats::{ns_to_ms, GenerateStats};
use serde::{Deserialize, Serialize};

/// Ollama `/api/generate` request body. `stream` is always true; unset options
/// and an absent system prompt are omitted.
#[derive(Serialize)]
pub(crate) struct GenerateRequest<'a> {
    pub model: &'a str,
    pub prompt: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<GenerateOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<i32>,
    /// `think:true` routes a reasoning model's scratchpad into the response's `thinking` field
    /// so the harness can capture it; `think:false` DISABLES thinking on a thinking-by-default
    /// model (omitting the field would let it reason anyway, invisibly burning `num_predict`).
    /// `None` omits the field (backend default — used by non-eval callers and warm-up).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub think: Option<bool>,
    pub stream: bool,
}

/// One NDJSON chunk. The final (`done:true`) chunk also carries nanosecond
/// duration + token-count metrics, mapped to `GenerateStats` (ns→ms).
#[derive(Deserialize)]
pub(crate) struct GenerateChunk {
    #[serde(default)]
    pub response: String,
    /// A reasoning model's scratchpad delta when `think:true` was requested — Ollama streams it
    /// here, NOT in `response`. Captured so the harness isn't blind to (or mis-budgeting) the
    /// reasoning. Empty on non-reasoning turns / backends that don't split the channel.
    #[serde(default)]
    pub thinking: String,
    pub done: bool,
    /// Why generation stopped on the final chunk: `"stop"` (natural end) vs `"length"`
    /// (hit the `num_predict` cap → truncated). The agentic runner reads this to retry a
    /// truncated turn instead of scoring it as a capability failure.
    #[serde(default)]
    done_reason: Option<String>,
    #[serde(default)]
    load_duration: Option<u64>,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    prompt_eval_duration: Option<u64>,
    #[serde(default)]
    eval_count: Option<u32>,
    #[serde(default)]
    eval_duration: Option<u64>,
    #[serde(default)]
    total_duration: Option<u64>,
}

impl GenerateChunk {
    pub(crate) fn stats(&self) -> GenerateStats {
        GenerateStats {
            prompt_eval_count: self.prompt_eval_count,
            prompt_eval_ms: self.prompt_eval_duration.map(ns_to_ms),
            eval_count: self.eval_count,
            eval_ms: self.eval_duration.map(ns_to_ms),
            load_ms: self.load_duration.map(ns_to_ms),
            total_ms: self.total_duration.map(ns_to_ms),
            cache_n: None, // Ollama's /api/generate reports no prompt-cache count
            finish_reason: self.done_reason.clone(), // "stop" | "length"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_alive_serializes_when_set_and_is_omitted_when_none() {
        let with = GenerateRequest {
            model: "m", prompt: "p", system: None, options: None,
            keep_alive: Some(-1), think: None, stream: true,
        };
        let json = serde_json::to_string(&with).unwrap();
        assert!(json.contains("\"keep_alive\":-1"), "{json}");

        let without = GenerateRequest {
            model: "m", prompt: "p", system: None, options: None,
            keep_alive: None, think: None, stream: true,
        };
        assert!(!serde_json::to_string(&without).unwrap().contains("keep_alive"));
    }

    /// `think` is sent BOTH ways for eval turns — `true` splits the scratchpad for capture,
    /// `false` disables a thinking-by-default model — and omitted only when `None` (non-eval
    /// callers / warm-up keep the pre-field request byte-identical).
    #[test]
    fn think_serializes_both_values_and_is_omitted_when_none() {
        let thinking = GenerateRequest {
            model: "m", prompt: "p", system: None, options: None,
            keep_alive: None, think: Some(true), stream: true,
        };
        assert!(serde_json::to_string(&thinking).unwrap().contains("\"think\":true"));

        let suppressed = GenerateRequest {
            model: "m", prompt: "p", system: None, options: None,
            keep_alive: None, think: Some(false), stream: true,
        };
        assert!(serde_json::to_string(&suppressed).unwrap().contains("\"think\":false"));

        let plain = GenerateRequest {
            model: "m", prompt: "p", system: None, options: None,
            keep_alive: None, think: None, stream: true,
        };
        assert!(!serde_json::to_string(&plain).unwrap().contains("think"));
    }

    /// A chunk carrying Ollama's split `thinking` channel deserializes it; an old-shape chunk
    /// (no `thinking` key) defaults to empty — back-compat.
    #[test]
    fn chunk_captures_thinking_and_defaults_empty() {
        let c: GenerateChunk =
            serde_json::from_str(r#"{"response":"","thinking":"let me reason","done":false}"#).unwrap();
        assert_eq!(c.thinking, "let me reason");
        assert_eq!(c.response, "");
        let old: GenerateChunk = serde_json::from_str(r#"{"response":"hi","done":true}"#).unwrap();
        assert_eq!(old.thinking, "", "absent thinking key defaults empty");
    }
}
