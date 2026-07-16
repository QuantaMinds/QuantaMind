use crate::inference::generate::generate_stats::GenerateStats;
use serde::Deserialize;

/// llama-server's `timings` object on the final (`stop:true`) chunk. Durations
/// are already milliseconds (f64); counts are token tallies. llama.cpp reports
/// no model-load time, so `load_ms`/`total_ms` stay `None`.
#[derive(Deserialize, Default)]
pub struct Timings {
    #[serde(default)]
    pub prompt_n: Option<u32>,
    #[serde(default)]
    pub prompt_ms: Option<f64>,
    #[serde(default)]
    pub predicted_n: Option<u32>,
    #[serde(default)]
    pub predicted_ms: Option<f64>,
    /// Prompt tokens served from cache (prefix reuse). High on a multi-turn
    /// agentic run whose transcript prefix was reused (prefill ≈ 0).
    #[serde(default)]
    pub cache_n: Option<u32>,
}

impl Timings {
    /// `finish_reason` is a REQUIRED argument, not a field this can fill: llama-server puts
    /// it on the `choice`, not in `timings`, so only the caller has it.
    ///
    /// It used to be `None` here with a comment saying "set by the caller" — and of the two
    /// callers, the streaming one did and the TOOL one silently didn't. `stats.finish_reason`
    /// was therefore permanently `None` on every llama.cpp native turn, which made the
    /// runner's truncation retry and its setting-vs-hardware split (D9) DEAD CODE: every
    /// truncated tool turn was laundered into `Malformed`/`Hallucinated`/`EmptyOutput` —
    /// blaming the model for our own decoder's gap. A comment can't enforce a contract, so
    /// this is a parameter: you cannot build these stats without answering the question.
    pub fn stats(&self, finish_reason: Option<String>) -> GenerateStats {
        GenerateStats {
            prompt_eval_count: self.prompt_n,
            prompt_eval_ms: self.prompt_ms.map(|m| m.round() as u64),
            eval_count: self.predicted_n,
            eval_ms: self.predicted_ms.map(|m| m.round() as u64),
            load_ms: None,
            total_ms: None,
            cache_n: self.cache_n,
            finish_reason,
            native_tool_calls: None, // only `NativeToolTurn` asks the native tool API
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_to_prefill_stats_rounding_ms_load_is_none() {
        let t = Timings {
            prompt_n: Some(128), prompt_ms: Some(210.7),
            predicted_n: Some(42), predicted_ms: Some(900.2),
            cache_n: Some(64),
        };
        // The stop reason comes from the caller (the choice), not from `timings` — pass it
        // through and assert it survives, since dropping it here is the whole bug.
        let stats = t.stats(Some("length".into()));
        assert_eq!(stats.finish_reason.as_deref(), Some("length"), "the caller's stop reason must survive");
        assert_eq!(stats.prompt_eval_count, Some(128));
        assert_eq!(stats.prompt_eval_ms, Some(211)); // rounded
        assert_eq!(stats.eval_count, Some(42));
        assert_eq!(stats.load_ms, None);
        assert_eq!(stats.cache_n, Some(64)); // prompt-cache reuse carried through
    }
}
