use crate::inference::generate::generate_stats::GenerateStats;
use crate::inference::openai::chat_chunk::Usage;

/// Map an OpenAI-compatible `usage` block to `GenerateStats`. These servers report
/// token counts only — no per-phase timing — so every `*_ms` field stays `None`
/// ("not available"); absent usage yields the all-`None` default. TTFT and
/// tokens/sec come from the client-side `RunTiming`, not from here.
pub fn from_usage(usage: Option<Usage>) -> GenerateStats {
    let u = usage.unwrap_or_default();
    GenerateStats {
        prompt_eval_count: u.prompt_tokens,
        eval_count: u.completion_tokens,
        prompt_eval_ms: None,
        eval_ms: None,
        load_ms: None,
        total_ms: None,
        cache_n: None,
        finish_reason: None, // set by the caller from the chunk's choice.finish_reason
        // `native_tool_calls` stays defaulted (None): this decodes a plain generate/usage
        // payload, which never asked the native tool API — a zero here would be a claim.
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_maps_token_counts_leaves_times_none() {
        let u = Usage { prompt_tokens: Some(12), completion_tokens: Some(30), total_tokens: Some(42) };
        let s = from_usage(Some(u));
        assert_eq!(s.prompt_eval_count, Some(12));
        assert_eq!(s.eval_count, Some(30));
        assert!(s.prompt_eval_ms.is_none() && s.eval_ms.is_none());
        assert!(s.load_ms.is_none() && s.total_ms.is_none());
    }

    #[test]
    fn absent_usage_yields_all_none() {
        assert_eq!(from_usage(None), GenerateStats::default());
    }
}
