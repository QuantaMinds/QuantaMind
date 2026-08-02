use crate::inference::generate::generate_stats::GenerateStats;
use serde_json::Value;

/// One native tool call, neutral of eval types: a name + a real argument object.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeToolCall {
    pub name: String,
    pub args: Value,
}

/// The translated native-tool-call result: the real `tool_calls`, the assistant
/// `content` (for the caller's abstain check), and token stats.
pub struct ChatResult {
    pub tool_calls: Vec<NativeToolCall>,
    pub content: String,
    pub stats: GenerateStats,
}

/// Normalize a tool-call `arguments` value: some models return it as a JSON
/// *string* rather than an object — parse it back so the canonical args are a
/// real object (checkpoint/arg matching compares objects, not quoted strings).
/// Shared by every native backend, whose builds disagree on string-vs-object args.
pub fn normalize_args(v: Value) -> Value {
    match v {
        Value::String(s) => serde_json::from_str(&s).unwrap_or(Value::String(s)),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_arguments_are_parsed_back_into_an_object() {
        let got = normalize_args(Value::String(r#"{"id":"I-2","amount":520}"#.into()));
        assert_eq!(got, serde_json::json!({"id": "I-2", "amount": 520}));
    }

    #[test]
    fn a_real_object_passes_through_untouched() {
        let obj = serde_json::json!({"id": "I-2"});
        assert_eq!(normalize_args(obj.clone()), obj);
    }

    #[test]
    fn a_non_json_string_stays_a_string_rather_than_erroring() {
        // Never lose the model's actual output to a parse failure.
        assert_eq!(normalize_args(Value::String("not json".into())), Value::String("not json".into()));
    }
}
