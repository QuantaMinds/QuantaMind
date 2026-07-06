use crate::inference::eval::toolcall::tasks::Call;
use serde_json::Value;

/// world_state keys that are NOT discoverable entities — meta/oracle data the
/// responder must never hand back as if it were an entity an arg pointed at, and
/// the generator must never alpha-rename. Single source of truth (generator and
/// the CI guards import it). Every key here is answer-key/scoring data authored
/// into `world_state` that no intended tool call fetches; leaving one out lets a
/// lucky arg guess exfiltrate the whole answer blob. The
/// `no_unfetched_world_state_key_is_resolvable_by_a_getter` guard in scenarios.rs
/// keeps this list exact: a bundled ws key must be reachable (prompt / another
/// entity's blob / expected-call arg / tool name) or listed here.
pub(crate) const RESERVED: &[&str] = &[
    "calc",
    "threshold",
    "ground_truth",
    "carbon_budget",
    "closing_rule",
    "compromised_cred",
    "deal_note",
    "defect",
    "egress_source",
    "expected_tax",
    "fail_test",
    "fake_coa",
    "fix_makes_green",
    "forbidden",
    "forensic_snapshot_required_before_change",
    "insider_suspect",
    "masking_fixes",
    "outcome",
    "pr_note",
    "rate",
    "real_bug",
    "root_cause_file",
    "root_cause_files",
    "rule",
    "rules",
    "secret_decoy",
    "spend_cap_pct",
    "sponsor_note",
    "stock",
    "valid_fix",
    "vents_free",
    "wrong_fix",
];

pub(crate) const ACK: &str = r#"{"ok":true}"#;

/// Derive a tool response from `world_state`. The model discovers ground truth by
/// calling tools; the sandbox returns the WHOLE entity sub-object for the first
/// string-valued arg whose value is a (non-reserved) world_state key — so every
/// tool on the same entity (`get_positions`, `compute_margin`, …) sees the same
/// blob and reads the field it needs (no per-tool projection). The `calc` sub-map
/// (`ws["calc"][expression]`) is handled first. A getter whose args carry no
/// entity key (computation/no-arg getters — `run_import_check{}`,
/// `convert_temp{k:…}`) falls back to the ws entry authored under the TOOL's own
/// name — the same whole-blob convention, keyed by tool instead of entity, so a
/// tool called with different args gets one blob holding every answer and reads
/// its field. A call that resolves to nothing gets a generic ack (it still can't
/// advance any checkpoint).
pub fn derive_response(ws: &Value, call: &Call) -> String {
    let Some(args) = call.args.as_object() else {
        return ACK.to_string();
    };

    // calc sub-map: an arg value that keys into ws["calc"] returns its result.
    if let Some(calc) = ws.get("calc").and_then(Value::as_object) {
        for v in args.values() {
            if let Some(s) = v.as_str() {
                if let Some(hit) = calc.get(s) {
                    return hit.to_string();
                }
            }
        }
    }

    // Entity resolution: first string arg whose value is a non-reserved ws key.
    if let Some(ws_obj) = ws.as_object() {
        for v in args.values() {
            if let Some(s) = v.as_str() {
                if RESERVED.contains(&s) {
                    continue;
                }
                if let Some(entity) = ws_obj.get(s) {
                    return entity.to_string();
                }
            }
        }
        // Tool-name fallback: no arg keyed an entity → the blob authored under the
        // tool's own name (never a reserved meta key).
        if !RESERVED.contains(&call.name.as_str()) {
            if let Some(blob) = ws_obj.get(&call.name) {
                return blob.to_string();
            }
        }
    }

    ACK.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ws() -> Value {
        json!({
            "M-3": { "ratio": 0.1, "maint": 0.25, "hedged": false },
            "M-4": { "ratio": 0.1, "hedged": true, "net_after_hedge": 0.3 },
            "threshold": { "ctr": 10000 },
            "calc": { "100000*0.03/12": 250.0 },
            "outcome": { "M-3": "LIQUIDATE", "M-4": "NO_ACTION" }
        })
    }
    fn call(name: &str, args: Value) -> Call {
        Call { name: name.into(), args }
    }

    #[test]
    fn every_tool_on_an_entity_gets_the_whole_sub_object() {
        let positions = derive_response(&ws(), &call("get_positions", json!({ "account": "M-3" })));
        let margin = derive_response(&ws(), &call("compute_margin", json!({ "account": "M-3" })));
        assert_eq!(positions, margin); // same blob, no per-tool projection
        assert_eq!(positions, json!({ "ratio": 0.1, "maint": 0.25, "hedged": false }).to_string());
    }

    #[test]
    fn calc_sub_map_resolves_an_expression() {
        let r = derive_response(&ws(), &call("calc", json!({ "expression": "100000*0.03/12" })));
        assert_eq!(r, "250.0");
    }

    #[test]
    fn reserved_keys_are_not_treated_as_entities() {
        // An arg literally naming a reserved key must NOT return that meta blob —
        // neither the original trio nor the extended answer-key names (`outcome`
        // here holds the per-entity correct decision: the answer key itself).
        for key in ["threshold", "outcome", "rule", "expected_tax"] {
            let r = derive_response(&ws(), &call("peek", json!({ "x": key })));
            assert_eq!(r, ACK, "reserved key {key:?} leaked as an entity blob");
        }
    }

    #[test]
    fn unresolved_call_gets_a_generic_ack() {
        assert_eq!(derive_response(&ws(), &call("noop", json!({ "account": "ZZ" }))), ACK);
        assert_eq!(derive_response(&ws(), &call("noop", json!({}))), ACK);
    }

    #[test]
    fn a_no_arg_getter_resolves_via_its_tool_name_key() {
        let ws = json!({ "run_import_check": { "cycle": ["orders/service.py", "orders/notify.py"] } });
        let r = derive_response(&ws, &call("run_import_check", json!({})));
        assert_eq!(r, json!({ "cycle": ["orders/service.py", "orders/notify.py"] }).to_string());
    }

    #[test]
    fn a_computation_getter_falls_back_to_its_tool_name_blob() {
        // No arg value keys an entity (310.15 is a number, "C" isn't a ws key) → the
        // tool-name blob carries every answer; the model reads the field it asked for.
        let ws = json!({ "convert_temp": { "C": 37.0, "F": 98.6 } });
        assert_eq!(
            derive_response(&ws, &call("convert_temp", json!({ "k": 310.15, "to": "C" }))),
            json!({ "C": 37.0, "F": 98.6 }).to_string()
        );
    }

    #[test]
    fn entity_resolution_wins_over_the_tool_name_fallback() {
        let ws = json!({
            "M-3": { "ratio": 0.1 },
            "get_positions": { "should": "never win when an arg keys an entity" }
        });
        let r = derive_response(&ws, &call("get_positions", json!({ "account": "M-3" })));
        assert_eq!(r, json!({ "ratio": 0.1 }).to_string());
    }

    #[test]
    fn a_reserved_tool_name_never_returns_the_meta_blob() {
        // A tool literally named "calc" with an unresolved expression must ack, not
        // hand back the whole calc answer map.
        let r = derive_response(&ws(), &call("calc", json!({ "expression": "unknown" })));
        assert_eq!(r, ACK);
    }
}
