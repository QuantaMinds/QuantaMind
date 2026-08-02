use super::*;

#[test]
fn parses_a_content_delta() {
    let chunk: ChatChunk =
        serde_json::from_str(r#"{"choices":[{"delta":{"content":"He"}}]}"#).expect("parse");
    assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("He"));
    assert!(chunk.choices[0].finish_reason.is_none());
    assert!(chunk.usage.is_none());
}

#[test]
fn parses_terminal_chunk_with_usage() {
    let chunk: ChatChunk = serde_json::from_str(
        r#"{"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":3,"total_tokens":8}}"#,
    )
    .expect("parse");
    assert_eq!(chunk.choices[0].finish_reason.as_deref(), Some("stop"));
    assert!(chunk.choices[0].delta.content.is_none());
    let u = chunk.usage.expect("usage");
    assert_eq!(u.prompt_tokens, Some(5));
    assert_eq!(u.completion_tokens, Some(3));
}

#[test]
fn empty_choices_parse_without_error() {
    let chunk: ChatChunk = serde_json::from_str(r#"{"choices":[]}"#).expect("parse");
    assert!(chunk.choices.is_empty());
}

#[test]
fn strip_sse_removes_data_prefix_only_when_present() {
    assert_eq!(strip_sse(b"data: {\"x\":1}"), b"{\"x\":1}");
    assert_eq!(strip_sse(b"{\"x\":1}"), b"{\"x\":1}");
}

#[test]
fn reasoning_delta_accepts_both_field_names() {
    // mlx_lm.server uses `reasoning`; vLLM reasoning parsers use `reasoning_content`.
    let remote: ChatChunk =
        serde_json::from_str(r#"{"choices":[{"delta":{"reasoning":"hmm"}}]}"#).expect("parse");
    assert_eq!(remote.choices[0].delta.reasoning.as_deref(), Some("hmm"));
    let vllm: ChatChunk =
        serde_json::from_str(r#"{"choices":[{"delta":{"reasoning_content":"hmm"}}]}"#).expect("parse");
    assert_eq!(vllm.choices[0].delta.reasoning.as_deref(), Some("hmm"));
}
