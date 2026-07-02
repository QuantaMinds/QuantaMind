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
    pub stream: bool,
}

/// One NDJSON chunk. The final (`done:true`) chunk also carries nanosecond
/// duration + token-count metrics, mapped to `GenerateStats` (ns→ms).
#[derive(Deserialize)]
pub(crate) struct GenerateChunk {
    #[serde(default)]
    pub response: String,
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
            keep_alive: Some(-1), stream: true,
        };
        let json = serde_json::to_string(&with).unwrap();
        assert!(json.contains("\"keep_alive\":-1"), "{json}");

        let without = GenerateRequest {
            model: "m", prompt: "p", system: None, options: None,
            keep_alive: None, stream: true,
        };
        assert!(!serde_json::to_string(&without).unwrap().contains("keep_alive"));
    }
}
