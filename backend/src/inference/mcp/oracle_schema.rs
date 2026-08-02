//! Track A — the **schema oracle**. Answer-key-free: given a tool's MCP
//! `inputSchema` (from `tools/list`), decide whether a model's call conforms —
//! right tool name (not hallucinated), args match the schema. Works on ANY
//! server without knowing the user's task. This is deliberately a SEPARATE gate
//! from "correct": a schema-valid call can still be the wrong call.
//!
//! Validates a JSON-Schema subset that real MCP servers use (draft-07/2020-12):
//! `type`, `properties` (recursive), `required`, `enum`, `additionalProperties`
//! (reject hallucinated args), array `items`. Unknown keywords are lenient.

use crate::inference::chat::native_call::NativeToolCall;
use crate::mcp::registry::split_namespaced;
use crate::mcp::wire::Tool;
use serde_json::Value;

/// The outcome of checking one model call against the offered tools.
#[derive(Debug, Clone, PartialEq)]
pub enum CallCheck {
    /// Right tool, args conform to its schema.
    Valid,
    /// The model named a tool that isn't offered (hallucinated) — a model fault,
    /// caught client-side before it ever reaches the server as a `-32602`.
    UnknownTool,
    /// Right tool, but the args violate its schema.
    Invalid(Vec<String>),
}

impl CallCheck {
    pub fn is_valid(&self) -> bool {
        matches!(self, CallCheck::Valid)
    }
}

/// Check one call against the offered tools (names may be namespaced).
pub fn check_call(tools: &[Tool], call: &NativeToolCall) -> CallCheck {
    let name = split_namespaced(&call.name).map(|(_, t)| t).unwrap_or(call.name.as_str());
    let Some(tool) = tools.iter().find(|t| t.name == name) else {
        return CallCheck::UnknownTool;
    };
    let violations = validate_against_schema(&tool.input_schema, &call.args);
    if violations.is_empty() {
        CallCheck::Valid
    } else {
        CallCheck::Invalid(violations)
    }
}

/// Validate `args` against an MCP `inputSchema`. Returns the list of violations
/// (empty == valid).
pub fn validate_against_schema(input_schema: &Value, args: &Value) -> Vec<String> {
    let mut out = Vec::new();
    validate_value(input_schema, args, "args", &mut out);
    out
}

fn validate_value(schema: &Value, value: &Value, path: &str, out: &mut Vec<String>) {
    if let Some(ty) = schema.get("type").and_then(Value::as_str) {
        if !type_matches(ty, value) {
            out.push(format!("{path}: expected {ty}"));
            return; // a type mismatch makes deeper checks meaningless
        }
    }
    if let Some(en) = schema.get("enum").and_then(Value::as_array) {
        if !en.iter().any(|v| v == value) {
            out.push(format!("{path}: value not in enum"));
        }
    }
    if let Some(obj) = value.as_object() {
        if let Some(req) = schema.get("required").and_then(Value::as_array) {
            for r in req.iter().filter_map(Value::as_str) {
                if !obj.contains_key(r) {
                    out.push(format!("{path}.{r}: required but missing"));
                }
            }
        }
        let props = schema.get("properties").and_then(Value::as_object);
        // additionalProperties:false rejects hallucinated args (OpenAI strict mode).
        let allow_extra = !matches!(schema.get("additionalProperties"), Some(Value::Bool(false)));
        for (k, v) in obj {
            match props.and_then(|p| p.get(k)) {
                Some(prop_schema) => validate_value(prop_schema, v, &format!("{path}.{k}"), out),
                None if !allow_extra => {
                    out.push(format!("{path}.{k}: unexpected property (additionalProperties:false)"))
                }
                None => {}
            }
        }
    }
    if let (Some(items), Some(arr)) = (schema.get("items"), value.as_array()) {
        for (i, it) in arr.iter().enumerate() {
            validate_value(items, it, &format!("{path}[{i}]"), out);
        }
    }
}

fn type_matches(ty: &str, v: &Value) -> bool {
    match ty {
        "string" => v.is_string(),
        "number" => v.is_number(),
        "integer" => v.is_i64() || v.is_u64() || v.as_f64().is_some_and(|f| f.fract() == 0.0),
        "boolean" => v.is_boolean(),
        "object" => v.is_object(),
        "array" => v.is_array(),
        "null" => v.is_null(),
        _ => true, // unknown/absent type keyword → lenient
    }
}

/// Aggregate schema conformance over many calls — the Track A **schema-valid
/// rate** (a format-reliability metric, NOT a task-correctness one).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SchemaScore {
    pub total: usize,
    pub valid: usize,
    pub unknown_tool: usize,
    pub invalid: usize,
}

impl SchemaScore {
    pub fn add(&mut self, check: &CallCheck) {
        self.total += 1;
        match check {
            CallCheck::Valid => self.valid += 1,
            CallCheck::UnknownTool => self.unknown_tool += 1,
            CallCheck::Invalid(_) => self.invalid += 1,
        }
    }
    /// Fraction of calls that were schema-valid (0.0 when no calls).
    pub fn rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.valid as f64 / self.total as f64
        }
    }
}

pub fn score_calls(tools: &[Tool], calls: &[NativeToolCall]) -> SchemaScore {
    let mut s = SchemaScore::default();
    for c in calls {
        s.add(&check_call(tools, c));
    }
    s
}

#[cfg(test)]
#[path = "oracle_schema_tests.rs"]
mod tests;
