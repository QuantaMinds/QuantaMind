use super::*;

fn opts() -> GenerateOptions {
    GenerateOptions {
        num_predict: Some(16),
        temperature: Some(0.2),
        top_k: Some(40),
        repeat_penalty: Some(1.1),
        seed: Some(7),
        ..Default::default()
    }
}

#[test]
fn maps_num_predict_to_max_tokens() {
    let json = serde_json::to_string(&ChatRequest::new("m".into(), "hi".into(), None, Some(opts()), None))
        .expect("serialize");
    assert!(json.contains("\"max_tokens\":16"));
    assert!(!json.contains("num_predict"));
}

#[test]
fn maps_top_k_and_repeat_penalty_to_repetition_penalty() {
    let json = serde_json::to_string(&ChatRequest::new("m".into(), "hi".into(), None, Some(opts()), None))
        .expect("serialize");
    assert!(json.contains("\"top_k\":40"));
    assert!(json.contains("\"repetition_penalty\":1.1"));
    assert!(!json.contains("repeat_penalty"));
}

#[test]
fn does_not_send_seed() {
    let json = serde_json::to_string(&ChatRequest::new("m".into(), "hi".into(), None, Some(opts()), None))
        .expect("serialize");
    assert!(!json.contains("seed"));
}

#[test]
fn serializes_stream_true_and_model() {
    let json = serde_json::to_string(&ChatRequest::new("phi".into(), "hi".into(), None, None, None))
        .expect("serialize");
    assert!(json.contains("\"stream\":true"));
    assert!(json.contains("\"model\":\"phi\""));
}

#[test]
fn requests_usage_on_the_stream() {
    // vLLM omit `usage` from streamed chunks unless include_usage is set
    // (verified live — token counts came back None without it).
    let json = serde_json::to_string(&ChatRequest::new("m".into(), "hi".into(), None, None, None))
        .expect("serialize");
    assert!(json.contains("\"stream_options\":{\"include_usage\":true}"));
}

#[test]
fn system_becomes_a_leading_message() {
    let req = ChatRequest::new("m".into(), "hi".into(), Some("be terse"), None, None);
    assert_eq!(req.messages.len(), 2);
    assert_eq!(req.messages[0].role, "system");
    assert_eq!(req.messages[0].content, "be terse");
    assert_eq!(req.messages[1].role, "user");
}

#[test]
fn empty_system_is_omitted() {
    let req = ChatRequest::new("m".into(), "hi".into(), Some(""), None, None);
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
}

#[test]
fn enable_thinking_reflects_the_think_flag_both_ways() {
    // is_thinking=true → capture reasoning; anything else → truly non-thinking (suppress a
    // has_thinking model's default reasoning so the smaller non-thinking budget isn't blown).
    let on = serde_json::to_string(&ChatRequest::new("m".into(), "hi".into(), None, None, Some(true))).unwrap();
    assert!(on.contains("\"chat_template_kwargs\":{\"enable_thinking\":true}"));
    for off in [None, Some(false)] {
        let json = serde_json::to_string(&ChatRequest::new("m".into(), "hi".into(), None, None, off)).unwrap();
        assert!(json.contains("\"enable_thinking\":false"), "think={off:?} must suppress reasoning");
    }
}
