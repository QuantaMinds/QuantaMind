use crate::inference::eval::toolcall::tasks::Call;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Which tool-call surface syntax a turn was parsed from. Models that ignore the
/// instructed JSON and emit their own native tool grammar (gemma's channel format)
/// would otherwise score as `MalformedJson` forever; we normalize the known dialects
/// to canonical `Call`s and record WHICH dialect so the UI can flag that the model
/// needed normalization (transparency — the score isn't silently laundered).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallDialect {
    /// The instructed format: JSON `{"name", "args"|"arguments"}` object(s). Not flagged.
    #[default]
    Standard,
    /// Channel/harmony syntax: `call:NAME{ bare: "args" }` wrapped in `<channel|>` /
    /// `<tool_call|>` tokens, with JS-style unquoted keys (gemma-style).
    Harmony,
}

fn strip_fences(s: &str) -> String {
    s.replace("```json", "").replace("```", "")
}

/// Every top-level balanced `{…}` slice in `text` (string/escape aware). Nested
/// objects stay inside their parent; array brackets are ignored (not braces).
fn objects(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let (mut depth, mut start, mut in_str, mut esc) = (0i32, 0usize, false, false);
    let mut out = Vec::new();
    for (i, &b) in bytes.iter().enumerate() {
        let c = b as char;
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => {
                if depth == 0 {
                    start = i;
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    out.push(&text[start..=i]);
                }
            }
            _ => {}
        }
    }
    out
}

fn to_call(v: Value) -> Option<Call> {
    let obj = v.as_object()?;
    let name = obj.get("name")?.as_str()?.to_string();
    let args = obj
        .get("args")
        .or_else(|| obj.get("arguments"))
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));
    Some(Call { name, args })
}

/// Greedily extract every parseable top-level JSON object and map the ones that
/// look like a tool call (`{name, args|arguments}`) to `Call`. Handles a single
/// object, a JSON array (the brackets aren't braces, so inner objects are
/// found), AND bare `{..}\n{..}` sequences that small quants emit. Non-parsing
/// brace text is discarded. Identical (name+args) calls are collapsed so a
/// chatty model that prints its call inline AND echoes it in a trailing fence
/// isn't wrongly failed by the cardinality guard; distinct parallel calls stay.
/// `None` when no call is found — so abstention is scoreable.
pub fn extract_calls(completion: &str) -> Option<Vec<Call>> {
    extract_calls_dialect(completion).map(|(calls, _)| calls)
}

/// Like [`extract_calls`] but also reports the dialect the calls were recovered from.
/// The instructed JSON is tried first; only when it yields nothing do we fall back to a
/// model-native dialect (so a clean JSON model is never mislabeled). The runner threads
/// the dialect into the trajectory so the UI can surface it.
pub fn extract_calls_dialect(completion: &str) -> Option<(Vec<Call>, ToolCallDialect)> {
    if let Some(calls) = extract_standard(completion) {
        return Some((calls, ToolCallDialect::Standard));
    }
    let harmony = harmony_calls(completion);
    (!harmony.is_empty()).then_some((harmony, ToolCallDialect::Harmony))
}

/// The instructed path: every parseable top-level JSON object mapped to a `Call`.
fn extract_standard(completion: &str) -> Option<Vec<Call>> {
    let cleaned = strip_fences(completion);
    let mut calls: Vec<Call> = Vec::new();
    for call in objects(&cleaned)
        .into_iter()
        .filter_map(|slice| serde_json::from_str::<Value>(slice).ok())
        .filter_map(to_call)
    {
        if !calls.contains(&call) {
            calls.push(call);
        }
    }
    (!calls.is_empty()).then_some(calls)
}

/// Recover calls from the harmony/channel dialect: each `call:NAME{ … }` becomes a `Call`
/// with NAME as the tool and the (relaxed) brace body as `args`. Channel tokens around it
/// are ignored — we anchor only on the `call:IDENT{` signature, strong enough that prose
/// almost never trips it. The body is JS-object-ish (unquoted keys, raw newlines in code
/// strings), so it goes through `relax_object`, not strict serde.
fn harmony_calls(text: &str) -> Vec<Call> {
    let chars: Vec<char> = text.chars().collect();
    const MARK: [char; 5] = ['c', 'a', 'l', 'l', ':'];
    let mut calls: Vec<Call> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i..].starts_with(&MARK) {
            i += 1;
            continue;
        }
        let mut j = i + MARK.len();
        let name_start = j;
        while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
            j += 1;
        }
        let name: String = chars[name_start..j].iter().collect();
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        if name.is_empty() || j >= chars.len() || chars[j] != '{' {
            i += 1;
            continue;
        }
        match balanced_brace(&chars, j) {
            Some(end) => {
                let body: String = chars[j..=end].iter().collect();
                if let Some(args) = relax_object(&body) {
                    let call = Call { name, args };
                    if !calls.contains(&call) {
                        calls.push(call);
                    }
                }
                i = end + 1;
            }
            None => i += 1,
        }
    }
    calls
}

/// Index of the `}` that closes the `{` at `start`, string/escape aware (so braces and
/// quotes inside a string value don't skew the depth). `None` if unbalanced.
fn balanced_brace(chars: &[char], start: usize) -> Option<usize> {
    let (mut depth, mut in_str, mut esc) = (0i32, false, false);
    for (k, &c) in chars.iter().enumerate().skip(start) {
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
    }
    None
}

/// Parse a JS-object-ish brace body into JSON. Tries strict serde first (cheap); on
/// failure relaxes the two things model-native bodies violate — unquoted keys and raw
/// control chars inside string values — then re-parses. Returns `None` if still invalid
/// (e.g. a bareword value), so a genuinely broken body is dropped, not guessed.
fn relax_object(body: &str) -> Option<Value> {
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        return Some(v);
    }
    serde_json::from_str(&relax_to_json(body)).ok()
}

/// Single string-aware pass: quote bare object keys (an identifier in key position) and
/// escape raw control chars inside strings. Conservative — values that aren't strings,
/// numbers, or nested objects are passed through untouched (and may still fail to parse).
fn relax_to_json(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 16);
    let mut in_str = false;
    let mut esc = false;
    let mut expect_key = false; // we just passed a `{` or `,` → next identifier is a key
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            if esc {
                out.push(c);
                esc = false;
            } else {
                match c {
                    '\\' => {
                        out.push(c);
                        esc = true;
                    }
                    '"' => {
                        out.push(c);
                        in_str = false;
                    }
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
                    _ => out.push(c),
                }
            }
            i += 1;
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                expect_key = false;
                out.push(c);
                i += 1;
            }
            '{' | ',' => {
                expect_key = true;
                out.push(c);
                i += 1;
            }
            ':' => {
                expect_key = false;
                out.push(c);
                i += 1;
            }
            _ if c.is_whitespace() => {
                out.push(c);
                i += 1;
            }
            _ if expect_key && (c.is_alphabetic() || c == '_') => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                out.push('"');
                out.extend(&chars[start..i]);
                out.push('"');
                expect_key = false;
            }
            _ => {
                expect_key = false;
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// Does the text contain at least one balanced `{…}` slice that parses as JSON?
/// The agentic runner uses this to tell a structured fake-completion (valid JSON
/// object, just no `name` → a `task_complete` claim) from broken-JSON noise.
pub(crate) fn has_json_object(text: &str) -> bool {
    let cleaned = strip_fences(text);
    objects(&cleaned).into_iter().any(|s| serde_json::from_str::<Value>(s).is_ok())
}

/// A yield turn whose text has a `{` but no parseable JSON object — the model
/// tried to emit a call and produced broken JSON. Pure prose, or valid-but-not-a
/// -call JSON, is NOT broken (that's a hallucinated completion).
pub(crate) fn looks_like_broken_json(text: &str) -> bool {
    text.contains('{') && !has_json_object(text)
}

/// A yield with NO usable answer — empty, whitespace, or only punctuation/symbols (e.g. a
/// lone `.` a model emits right before its stop token, or `""` from a native turn that
/// returned no call and no content). A generation/template artifact (the model produced
/// nothing), distinct from a `Hallucinated` completion, which claims "done" in real words.
/// Keyed on the ABSENCE of any alphanumeric character so a real prose answer (or foreign
/// `call:` soup, which contains the tool name) is never swept in.
pub(crate) fn is_empty_output(text: &str) -> bool {
    !text.chars().any(|c| c.is_alphanumeric())
}

/// `call:IDENT` immediately followed (after optional whitespace) by `{` or `(` — the
/// SHAPE of an attempted tool call in the channel/harmony dialect, regardless of whether
/// the body parses. Mirrors `harmony_calls`'s anchor but also accepts the paren form and
/// does not require a parseable body, so it recognizes an *attempt* the normalizer dropped.
fn has_call_structure(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    const MARK: [char; 5] = ['c', 'a', 'l', 'l', ':'];
    let mut i = 0;
    while i < chars.len() {
        if !chars[i..].starts_with(&MARK) {
            i += 1;
            continue;
        }
        let mut j = i + MARK.len();
        let name_start = j;
        while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
            j += 1;
        }
        if j > name_start {
            let mut k = j;
            while k < chars.len() && chars[k].is_whitespace() {
                k += 1;
            }
            if k < chars.len() && (chars[k] == '{' || chars[k] == '(') {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// A no-call yield whose text is genuine foreign tool-call DIALECT soup — the model
/// generated in a non-JSON tool grammar (a mis-built gemma-qat emits harmony-ish channel
/// tokens) the parser can't read, rather than fumbling JSON ([`looks_like_broken_json`] →
/// `Malformed`) or yielding nothing (`Hallucinated`). Deliberately NOT salvaged: a real
/// deployment (Ollama's native parser) also drops these forms, so crediting them would make
/// the bench more lenient than production. Anchored on BOTH a channel control token AND an
/// attempted [`has_call_structure`]: either alone appears in ordinary prose (a model quoting
/// `<|tool_response|>`, or writing "call:foo(x)"), so the verdict requires the pair — they
/// only co-occur when the model is actually emitting the dialect. A bare `call:x{}` with no
/// channel token stays in the existing `Malformed`/`Hallucinated` buckets (conservative).
pub(crate) fn looks_like_foreign_dialect(text: &str) -> bool {
    let has_channel_token =
        text.contains("<|tool_response") || text.contains("<channel|") || text.contains("<tool_call|");
    has_channel_token && has_call_structure(text)
}

/// Remove every `<think>…</think>` reasoning span from a model turn so the tool-call
/// parser and the transcript see the model's ACTUAL output, not its scratchpad. A
/// reasoning model emits its chain-of-thought before the call; left in place its inner
/// `{…}` would be mis-parsed as a tool call (the `objects()` scanner has no notion of
/// think tags), and re-sending it every step would bloat the KV-cache prefix. Properties:
/// removes ALL spans (models interleave think → text → think → call, not just one); a
/// truncated, never-closed trailing `<think>` (cut mid-thought by the token cap) drops
/// from that tag to the end — if nothing remains, the run is an honest no-call yield, not
/// a malformed one; and a span whose body contains JSON-like braces is removed whole, so
/// the answer that follows parses cleanly. Only applied for thinking-tagged models.
pub fn strip_think(text: &str) -> String {
    const OPEN: &str = "<think>";
    const CLOSE: &str = "</think>";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find(OPEN) {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + OPEN.len()..];
        match after_open.find(CLOSE) {
            Some(close) => rest = &after_open[close + CLOSE.len()..],
            // Unclosed: truncated mid-thought. Drop from `<think>` to the end.
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Strip the `<think>` scratchpad before parsing tool calls, applied when the model is FLAGGED
/// thinking OR when the output actually CONTAINS a `<think>` tag. The second clause is dynamic
/// detection: a model that reasons but was NOT flagged thinking (a mis-set/absent toggle, e.g.
/// qwen2.5-coder) still has its tool call parsed from the answer beneath the reasoning, instead
/// of the scratchpad's braces being mis-scored as ForeignDialect/EmptyOutput. A terse model with
/// no `<think>` is byte-for-byte unchanged (the non-thinking path stays identical).
pub fn think_stripped(raw: &str, is_thinking: bool) -> String {
    if is_thinking || raw.contains("<think>") {
        strip_think(raw)
    } else {
        raw.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_clean_json_object() {
        let calls = extract_calls("{\"name\":\"get_weather\",\"args\":{\"city\":\"Paris\"}}").unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "get_weather");
        assert_eq!(calls[0].args, json!({"city":"Paris"}));
    }

    #[test]
    fn extracts_from_markdown_fence() {
        let calls = extract_calls("Sure:\n```json\n{\"name\":\"get_time\",\"args\":{\"timezone\":\"Tokyo\"}}\n```").unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "get_time");
    }

    #[test]
    fn extracts_parallel_array() {
        let calls = extract_calls("[{\"name\":\"a\",\"args\":{}},{\"name\":\"b\",\"args\":{}}]").unwrap();
        assert_eq!(calls.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn extracts_two_sequential_objects_without_an_array() {
        // Small quants emit bare sequential objects, no enclosing array.
        let calls = extract_calls("{\"name\":\"a\",\"args\":{}}\n{\"name\":\"b\",\"args\":{}}").unwrap();
        assert_eq!(calls.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn nested_args_object_captured_whole() {
        let calls = extract_calls("call: {\"name\":\"x\",\"args\":{\"a\":{\"b\":1}}} done").unwrap();
        assert_eq!(calls[0].args, json!({"a":{"b":1}}));
    }

    #[test]
    fn collapses_inline_call_echoed_in_a_fence() {
        // Chatty model: the call inline, then prose, then the SAME call re-stated
        // in a ```json block. After de-fencing both copies survive as objects —
        // dedup keeps exactly one so the cardinality guard doesn't false-fail.
        let calls = extract_calls(
            "[{\"name\":\"get_weather\",\"args\":{\"city\":\"Paris\"}}]\nHere's why:\n```json\n{\"name\":\"get_weather\",\"args\":{\"city\":\"Paris\"}}\n```",
        )
        .unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "get_weather");
        assert_eq!(calls[0].args, json!({"city":"Paris"}));
    }

    #[test]
    fn distinct_parallel_calls_are_not_collapsed() {
        // Same tool, different args → genuine parallel calls, both kept.
        let calls =
            extract_calls("[{\"name\":\"a\",\"args\":{\"x\":1}},{\"name\":\"a\",\"args\":{\"x\":2}}]").unwrap();
        assert_eq!(calls.len(), 2);
    }

    #[test]
    fn none_on_prose_only_and_malformed_json() {
        assert!(extract_calls("I can't help with that, sorry.").is_none());
        // Bareword VALUE (`get_weather`) can't be relaxed → still unparseable → None.
        assert!(extract_calls("{name: get_weather, city: Paris}").is_none());
    }

    #[test]
    fn clean_json_reports_the_standard_dialect() {
        let (_, dialect) = extract_calls_dialect(r#"{"name":"a","args":{}}"#).unwrap();
        assert_eq!(dialect, ToolCallDialect::Standard);
    }

    #[test]
    fn harmony_channel_calls_are_normalized_with_the_tool_name_outside_the_brace() {
        // Real gemma output: `call:NAME{ bare: "args" }` in channel tokens. The tool is
        // the identifier after `call:`; the (unquoted-key) brace body is its args.
        let raw = concat!(
            "thought\n<channel|><|tool_response>call:search_symbol{name: \"rounding_helper\"}",
            "<tool_call|><|tool_response>call:add_marker{marker: \"flaky\",test: \"test_webhook_latency\"}<tool_call|>"
        );
        let (calls, dialect) = extract_calls_dialect(raw).unwrap();
        assert_eq!(dialect, ToolCallDialect::Harmony);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "search_symbol");
        assert_eq!(calls[0].args, json!({"name": "rounding_helper"}));
        assert_eq!(calls[1].name, "add_marker");
        assert_eq!(calls[1].args, json!({"marker": "flaky", "test": "test_webhook_latency"}));
    }

    #[test]
    fn harmony_relaxes_unquoted_keys_and_raw_newlines_in_a_code_string() {
        // write_file with a multi-line python body: unquoted keys AND a literal newline
        // inside the string — both relaxed so the args round-trip exactly.
        let raw = "call:write_file{content: \"def round_to_currency(amount, precision=2):\n    return round(amount, precision)\",path: \"billing/utils/rounding_helper.py\"}";
        let (calls, dialect) = extract_calls_dialect(raw).unwrap();
        assert_eq!(dialect, ToolCallDialect::Harmony);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "write_file");
        assert_eq!(
            calls[0].args,
            json!({
                "content": "def round_to_currency(amount, precision=2):\n    return round(amount, precision)",
                "path": "billing/utils/rounding_helper.py"
            })
        );
    }

    #[test]
    fn standard_json_wins_over_harmony_when_both_could_match() {
        // A model that emits valid JSON AND happens to mention `call:x{}` in prose is
        // Standard — the instructed path is tried first and short-circuits.
        let (calls, dialect) =
            extract_calls_dialect("{\"name\":\"transfer\",\"args\":{\"amount\":1}} (i.e. call:transfer{amount:1})").unwrap();
        assert_eq!(dialect, ToolCallDialect::Standard);
        assert_eq!(calls[0].name, "transfer");
    }

    #[test]
    fn think_stripped_strips_dynamically_when_output_has_think_even_if_not_flagged() {
        // The mis-flag case: a reasoning model NOT flagged thinking (is_thinking=false) still
        // emits <think>. We must strip it so the tool call underneath parses, not treat the
        // scratchpad's braces as soup.
        let raw = "<think>let me reason</think>{\"name\":\"go\",\"args\":{}}";
        let cleaned = think_stripped(raw, false);
        assert!(!cleaned.contains("<think>"));
        assert_eq!(extract_calls(&cleaned).unwrap()[0].name, "go");
    }

    #[test]
    fn think_stripped_leaves_a_terse_no_think_output_byte_identical() {
        // A model with no <think> and not flagged thinking is unchanged (the non-thinking path
        // stays byte-for-byte identical — no accidental behavior change).
        let raw = "{\"name\":\"go\",\"args\":{}}";
        assert_eq!(think_stripped(raw, false), raw);
        assert_eq!(think_stripped("plain prose answer", false), "plain prose answer");
    }

    #[test]
    fn think_stripped_still_strips_when_flagged_thinking() {
        let raw = "<think>x</think>done";
        assert_eq!(think_stripped(raw, true), "done");
    }

    #[test]
    fn strip_think_removes_a_closed_block_leaving_the_call_untouched() {
        let cleaned = strip_think("<think>let me reason</think>{\"name\":\"go\",\"args\":{}}");
        let calls = extract_calls(&cleaned).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "go");
    }

    #[test]
    fn strip_think_removes_all_interleaved_blocks() {
        // Reasoning models interleave think → text → think → call, not just one block.
        let cleaned = strip_think("<think>a</think>thinking out loud<think>b</think>{\"name\":\"go\",\"args\":{}}");
        assert!(!cleaned.contains("<think>") && !cleaned.contains("</think>"));
        let calls = extract_calls(&cleaned).unwrap();
        assert_eq!(calls.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["go"]);
    }

    #[test]
    fn strip_think_drops_an_unclosed_truncated_block_to_an_honest_no_call() {
        // Token cap cut the model off mid-thought: no closing tag, no call ever emitted.
        let cleaned = strip_think("<think>reasoning that never finished because we ran out of budd");
        assert!(cleaned.is_empty());
        // No call parsed → an honest no-call yield (Hallucinated), NOT broken JSON.
        assert!(extract_calls(&cleaned).is_none());
        assert!(!looks_like_broken_json(&cleaned));
    }

    #[test]
    fn strip_think_discards_json_like_braces_inside_a_think_block() {
        // Adversarial: the model reasons ABOUT the call it is about to make. The `foo`
        // braces live inside <think> and must never be parsed — only the real `bar` call.
        let cleaned =
            strip_think("<think>I'll call {\"name\":\"foo\",\"args\":{}}</think>{\"name\":\"bar\",\"args\":{}}");
        let calls = extract_calls(&cleaned).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "bar");
    }

    // ---- foreign-dialect detection (production-parity: flag, do NOT salvage) ----

    #[test]
    fn foreign_dialect_paren_form_is_not_salvaged_but_is_flagged() {
        // A mis-built gemma-qat (prompt path) emits paren-form calls in channel tokens.
        // Ollama's native parser does not recover paren-form function calls, so neither do
        // we — but it IS recognized as foreign dialect, not a bare hallucination.
        let raw = "<audio|>thought<channel|><|tool_response|>call:reply(text='The test suite for module \"cart\" failed: test_add_item_to_cart_success')<tool_call|>";
        assert!(extract_calls_dialect(raw).is_none(), "paren form must NOT be salvaged (production drops it)");
        assert!(looks_like_foreign_dialect(raw));
        // And it isn't mistaken for a broken-JSON attempt (no '{' anywhere).
        assert!(!looks_like_broken_json(raw));
    }

    #[test]
    fn foreign_dialect_brace_with_pseudo_quote_wrappers_is_not_salvaged_but_is_flagged() {
        // The `<|"|>` string-value wrappers break relax_object — Ollama's native parser
        // dropped this `reply` too, so we drop it as well and flag the run foreign.
        let raw = "thought\n<channel|><|tool_response|>call:reply{text:<|\"|>The test suite for module 'cart' failed: test_add_item_to_cart_success<|\"|>}<tool_call|>";
        assert!(extract_calls_dialect(raw).is_none(), "<|\"|>-wrapped body must NOT be salvaged");
        assert!(looks_like_foreign_dialect(raw));
    }

    #[test]
    fn clean_brace_harmony_call_is_still_salvaged_at_production_parity() {
        // `call:run_tests{module:'cart'}` has no `<|"|>` wrapper — Ollama's native parser
        // recovers it, so the existing harmony path must keep recovering it (unchanged).
        let raw = "<channel|><|tool_response|>call:run_tests{module: \"cart\"}<tool_call|>";
        let (calls, dialect) = extract_calls_dialect(raw).unwrap();
        assert_eq!(dialect, ToolCallDialect::Harmony);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "run_tests");
        assert_eq!(calls[0].args, json!({"module": "cart"}));
    }

    #[test]
    fn prose_mentioning_call_syntax_is_not_flagged_foreign() {
        // Adversarial false-positive: a model EXPLAINING the dialect in plain prose has a
        // call-structure-looking substring and a stray `<|"|>`, but no channel control
        // token — so it must NOT be labeled foreign dialect (nor salvaged).
        let prose = "To invoke it you'd write call:foo(x) or wrap the value in a <|\"|> delimiter.";
        assert!(!looks_like_foreign_dialect(prose));
        assert!(extract_calls_dialect(prose).is_none());
    }

    #[test]
    fn channel_token_alone_without_a_call_is_not_flagged_foreign() {
        // A model discussing harmony tokens in prose, with no attempted call, is not foreign.
        let prose = "Harmony models wrap output in a <|tool_response|> block before the answer.";
        assert!(!looks_like_foreign_dialect(prose));
    }

    #[test]
    fn standard_json_is_not_flagged_foreign() {
        assert!(!looks_like_foreign_dialect("{\"name\":\"run_tests\",\"args\":{\"module\":\"cart\"}}"));
    }

    // ---- empty-output detection (no usable answer; a generation artifact) ----

    #[test]
    fn lone_punctuation_and_blank_strings_are_empty_output() {
        // The real gemma-qat prompt-path symptom: a single "." before the stop token.
        assert!(is_empty_output("."));
        assert!(is_empty_output(""));
        assert!(is_empty_output("   \n\t "));
        assert!(is_empty_output("•")); // the UI's empty-render glyph
        assert!(is_empty_output("...")); // ellipsis-only
    }

    #[test]
    fn any_real_content_is_not_empty_output() {
        assert!(!is_empty_output("3")); // a bare number is a (wrong-channel) answer, not empty
        assert!(!is_empty_output("The suite failed."));
        // foreign soup carries the tool name → not empty (the foreign check owns it)
        assert!(!is_empty_output("<channel|>call:reply(text='x')<tool_call|>"));
        assert!(!is_empty_output("{\"name\":\"run_tests\",\"args\":{}}"));
    }
}
