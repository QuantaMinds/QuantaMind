use crate::inference::eval::agentic::sandbox::EndStateRule;
use crate::inference::eval::agentic::v2::r#match::MustNotCall;
use crate::inference::eval::agentic::v2::world_state::RESERVED;
use crate::inference::eval::toolcall::tasks::ToolTask;
use serde_json::Value;

/// Deterministic per-run seed from the model name + run index (FNV-1a). Same
/// `(model, run_index)` → identical instance (reproducibility); different
/// `run_index` → a different instance (contamination resistance across the k runs).
pub fn seed_for(model: &str, run_index: u32) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let feed = |h: &mut u64, b: u8| {
        *h ^= b as u64;
        *h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for b in model.bytes() {
        feed(&mut h, b);
    }
    feed(&mut h, b'|');
    for b in run_index.to_le_bytes() {
        feed(&mut h, b);
    }
    h
}

/// Build a fresh instance of a generated task by consistently renaming its
/// numbered entity ids (the `world_state` top-level keys that contain a digit) by a
/// seeded offset — applied across the prompt, world_state, end-state checkpoints,
/// and `must_not_call`. This is a bijective alpha-rename: the decision logic is
/// untouched (oracle-safe), but the surface ids differ per run, defeating
/// exact-instance memorization. A task with no numbered entities is returned
/// unchanged (replay fallback — honest: it runs, just not varied).
pub fn instantiate(base: &ToolTask, seed: u64) -> ToolTask {
    let mut t = base.clone();
    let Some(spec) = t.agentic.as_mut() else { return t };
    let Some(ws) = spec.world_state.as_ref() else { return t };

    let ids: Vec<String> = ws
        .as_object()
        .map(|o| {
            o.keys()
                .filter(|k| !RESERVED.contains(&k.as_str()) && k.chars().any(|c| c.is_ascii_digit()))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        return t; // nothing safely remappable → replay the worked instance
    }

    let offset = (seed % 90) + 10; // keeps ids readable and the mapping injective
    // Longest id first so "M-12" is matched before "M-1" during replacement.
    let mut map: Vec<(String, String)> = ids.iter().map(|id| (id.clone(), remap_id(id, offset))).collect();
    map.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    t.prompt = replace_ids(&t.prompt, &map);
    if let Some(w) = spec.world_state.as_mut() {
        *w = remap_value(w, &map);
    }
    match &mut spec.end_state {
        EndStateRule::RequireAll(cps) | EndStateRule::RequireSequence(cps) => {
            for cp in cps {
                cp.args = remap_value(&cp.args, &map);
            }
        }
        EndStateRule::RequireEndState(target) => {
            *target = remap_value(target, &map);
        }
        EndStateRule::ExpectAbstainingText => {}
    }
    for m in &mut spec.must_not_call {
        if let MustNotCall::Pair { args, .. } = m {
            *args = remap_value(args, &map);
        }
    }
    t
}

/// Shift the trailing-number suffix of an id by `offset` (e.g. "M-3" → "M-13"),
/// keeping the prefix. Ids with no trailing number get the offset appended.
fn remap_id(old: &str, offset: u64) -> String {
    let digits = old.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return format!("{old}{offset}");
    }
    let (prefix, num) = old.split_at(old.len() - digits);
    match num.parse::<u64>() {
        Ok(n) => format!("{prefix}{}", n + offset),
        Err(_) => format!("{old}{offset}"),
    }
}

/// Remap ids inside a JSON value via its serialized form (ids appear as quoted
/// strings or substrings of strings; whole-word replacement keeps it consistent).
fn remap_value(v: &Value, map: &[(String, String)]) -> Value {
    serde_json::from_str(&replace_ids(&v.to_string(), map)).unwrap_or_else(|_| v.clone())
}

/// Replace whole-word occurrences of each `old` id with its `new` value, in ONE
/// pass (no chaining: a mapped value is never re-matched). A match is whole-word
/// when bounded by non-alphanumeric chars on both sides, so "M-1" never matches
/// inside "M-12". `map` must be sorted longest-first.
fn replace_ids(text: &str, map: &[(String, String)]) -> String {
    let bytes = text.as_bytes();
    let alnum = |c: u8| c.is_ascii_alphanumeric();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let at_boundary = i == 0 || !alnum(bytes[i - 1]);
        if at_boundary {
            if let Some((old, new)) = map.iter().find(|(old, _)| {
                text[i..].starts_with(old.as_str()) && {
                    let end = i + old.len();
                    end >= text.len() || !alnum(bytes[end])
                }
            }) {
                out.push_str(new);
                i += old.len();
                continue;
            }
        }
        let ch = text[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::eval::agentic::spec::AgenticSpec;
    use crate::inference::eval::agentic::v2::collection::load_v2_collection;
    use serde_json::json;

    fn gen_task() -> ToolTask {
        let c = r#"{
          "name": "g", "domain": "d", "tier": "Hard", "pass_k": 16, "generated": true,
          "tasks": [{
            "id": "g1", "category": "agent_loop", "max_steps": 20, "max_recovery": 2,
            "prompt": "Handle M-1 and M-2 per policy.",
            "world_state": { "M-1": { "ratio": 0.4 }, "M-2": { "ratio": 0.1 } },
            "tools": [ { "name": "act", "params": { "id": "string" } } ],
            "expected_calls": [
              { "type": "call", "name": "act", "args": { "id": "M-1" } },
              { "type": "call", "name": "act", "args": { "id": "M-2" } }
            ],
            "must_not_call": [ { "name": "act", "args": { "id": "M-9" } } ]
          }]
        }"#;
        load_v2_collection(c).unwrap().into_iter().next().unwrap()
    }
    fn ws_keys(t: &ToolTask) -> Vec<String> {
        let s: &AgenticSpec = t.agentic.as_ref().unwrap();
        s.world_state.as_ref().unwrap().as_object().unwrap().keys().cloned().collect()
    }

    #[test]
    fn same_seed_is_byte_identical_and_remaps_consistently() {
        let base = gen_task();
        let a = instantiate(&base, 7);
        let b = instantiate(&base, 7);
        assert_eq!(a, b); // reproducible within a run

        // ids shifted by the same offset everywhere (prompt + world_state + checkpoints).
        let keys = ws_keys(&a);
        assert!(!keys.contains(&"M-1".to_string()) && keys.len() == 2);
        // The remapped keys appear in the prompt (consistent rename across surfaces).
        assert!(keys.iter().all(|k| a.prompt.contains(k.as_str())));
        assert!(!a.prompt.contains("M-1 ")); // the original standalone token is gone
    }

    #[test]
    fn different_run_index_yields_a_different_instance_still_satisfiable() {
        let base = gen_task();
        let a = instantiate(&base, seed_for("m", 0));
        let b = instantiate(&base, seed_for("m", 1));
        assert_ne!(ws_keys(&a), ws_keys(&b)); // novel across runs

        // The remapped expected_calls still reference the remapped world_state keys,
        // so the (renamed) oracle resolves — checkpoints' ids ∈ world_state keys.
        let spec = a.agentic.as_ref().unwrap();
        let keys = ws_keys(&a);
        if let EndStateRule::RequireAll(cps) = &spec.end_state {
            for cp in cps {
                let id = cp.args.get("id").and_then(Value::as_str).unwrap();
                assert!(keys.contains(&id.to_string()), "checkpoint id {id} not in remapped world_state");
            }
        }
    }

    #[test]
    fn a_task_without_numbered_entities_replays_unchanged() {
        let mut base = gen_task();
        // Replace world_state with non-numbered keys.
        base.agentic.as_mut().unwrap().world_state = Some(json!({ "cart": { "ok": true } }));
        let inst = instantiate(&base, 42);
        assert_eq!(inst.agentic.as_ref().unwrap().world_state, base.agentic.as_ref().unwrap().world_state);
    }

    #[test]
    fn remap_id_shifts_trailing_number() {
        assert_eq!(remap_id("M-3", 10), "M-13");
        assert_eq!(remap_id("AC-1", 5), "AC-6");
        assert_eq!(remap_id("plain", 7), "plain7");
    }
}
