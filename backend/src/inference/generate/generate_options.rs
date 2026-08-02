use serde::Serialize;

/// Anti-collapse repetition penalty applied as the eval harness's per-turn
/// default. Greedy eval (`temperature 0.0`) with no penalty lets loop-prone
/// quantized models (gpt-oss/gemma) run to the token cap repeating one phrase;
/// the penalty reshapes logits before the argmax so the looped token stops being
/// the max. A user-set UI value still overrides it (see `merge_eval_options`).
pub const EVAL_REPEAT_PENALTY: f32 = 1.1;

/// the server `/api/generate` `options` block. Field names mirror the server's
/// API (note `num_predict`, not `max_tokens`). Every field is optional so
/// unset knobs fall back to the server's own defaults.
#[derive(Serialize, Default, Clone, Debug, PartialEq)]
pub struct GenerateOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_predict: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    /// Context window. Larger = the model uses more of its window for long
    /// prompts, at the cost of more KV-cache memory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_ctx: Option<u32>,
    /// Stop sequences passed to the server (`options.stop`). For models whose end-of-turn
    /// markers aren't a plain EOS (harmony's `<|return|>`/`<|call|>`, gemma's
    /// `<end_of_turn>`), these are what actually halt generation — without them the model
    /// emits the markers as literal text and loops. Resolved per-model from the chat
    /// template (see `BackendTurn::run`); omitted from the request when empty/unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
}

impl GenerateOptions {
    pub fn is_empty(&self) -> bool {
        self.temperature.is_none() && self.top_p.is_none() && self.top_k.is_none()
            && self.num_predict.is_none() && self.repeat_penalty.is_none() && self.seed.is_none()
            && self.num_ctx.is_none() && self.stop.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn num_ctx_serializes_and_counts_as_non_empty() {
        let o = GenerateOptions { num_ctx: Some(32768), ..Default::default() };
        assert!(!o.is_empty());
        let json = serde_json::to_string(&o).unwrap();
        assert!(json.contains("\"num_ctx\":32768"), "{json}");
        // unset → omitted, and an all-None options is empty
        assert!(GenerateOptions::default().is_empty());
        assert!(!serde_json::to_string(&GenerateOptions::default()).unwrap().contains("num_ctx"));
    }

    #[test]
    fn stop_serializes_as_an_array_and_counts_as_non_empty() {
        let o = GenerateOptions { stop: Some(vec!["<|return|>".into(), "<|call|>".into()]), ..Default::default() };
        assert!(!o.is_empty());
        let json = serde_json::to_string(&o).unwrap();
        assert!(json.contains("\"stop\":[\"<|return|>\",\"<|call|>\"]"), "{json}");
        // unset → omitted entirely
        assert!(!serde_json::to_string(&GenerateOptions::default()).unwrap().contains("stop"));
    }
}
