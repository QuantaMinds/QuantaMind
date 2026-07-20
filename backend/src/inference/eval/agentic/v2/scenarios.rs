use serde::Deserialize;

/// Every bundled v2 tiered scenario collection: `(id, raw JSON)`. The id is the
/// file stem; the collection's domain + tier come from the JSON header. These are
/// THE eval content — they replace the old hand-coded single/multi fixtures.
pub const V2_SCENARIOS: &[(&str, &str)] = &[
    ("easy-coding", include_str!("scenarios/easy-coding.json")),
    ("easy-coding-fs", include_str!("scenarios/easy-coding-fs.json")),
    ("easy-research-search", include_str!("scenarios/easy-research-search.json")),
    ("easy-webui-tasks", include_str!("scenarios/easy-webui-tasks.json")),
    ("easy-customer-support", include_str!("scenarios/easy-customer-support.json")),
    ("easy-ecommerce", include_str!("scenarios/easy-ecommerce.json")),
    ("easy-finance", include_str!("scenarios/easy-finance.json")),
    ("easy-math-science", include_str!("scenarios/easy-math-science.json")),
    ("medium-coding", include_str!("scenarios/medium-coding.json")),
    ("medium-customer-support", include_str!("scenarios/medium-customer-support.json")),
    ("medium-ecommerce", include_str!("scenarios/medium-ecommerce.json")),
    ("medium-finance", include_str!("scenarios/medium-finance.json")),
    ("medium-legal", include_str!("scenarios/medium-legal.json")),
    ("medium-medical", include_str!("scenarios/medium-medical.json")),
    ("hard-coding", include_str!("scenarios/hard-coding.json")),
    ("hard-finance", include_str!("scenarios/hard-finance.json")),
    ("hard-finance-2", include_str!("scenarios/hard-finance-2.json")),
    ("hard-medical", include_str!("scenarios/hard-medical.json")),
    ("hard-support-ecommerce", include_str!("scenarios/hard-support-ecommerce.json")),
    ("extreme-clinical-trial-stats", include_str!("scenarios/extreme-clinical-trial-stats.json")),
    ("extreme-legal-compliance", include_str!("scenarios/extreme-legal-compliance.json")),
    ("extreme-supply-chain-recon", include_str!("scenarios/extreme-supply-chain-recon.json")),
    // Category K — safety/boundary probes (Attack + BenignControl arms per domain).
    ("boundary-healthcare", include_str!("scenarios/boundary-healthcare.json")),
    ("boundary-banking", include_str!("scenarios/boundary-banking.json")),
    ("boundary-coding", include_str!("scenarios/boundary-coding.json")),
    // Category K context-squeeze: the live proof of the GuardTruncatedByConfig attribution.
    ("boundary-context-squeeze", include_str!("scenarios/boundary-context-squeeze.json")),
    // Field-extraction under payload noise.
    ("noisy-extraction", include_str!("scenarios/noisy-extraction.json")),
];

/// Raw JSON for a bundled v2 collection by id.
pub fn v2_json(id: &str) -> Option<&'static str> {
    V2_SCENARIOS.iter().find(|(i, _)| *i == id).map(|(_, j)| *j)
}

/// Lightweight collection header for the picker (domain + tier), without
/// transpiling every task.
#[derive(Deserialize)]
pub struct V2Header {
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub tier: String,
}

pub fn v2_header(json: &str) -> Option<V2Header> {
    serde_json::from_str(json).ok()
}

/// Stable content hash of a bundled collection — lowercase-hex SHA-256 over its raw
/// JSON bytes — so the leaderboard only compares results measured on the *same*
/// scenario set (a pass^k on a v1 collection isn't comparable to an edited v2). The
/// `include_str!` bytes are deterministic, so the hash is identical across builds.
/// `None` for an unknown id, which the publish projection reads as "not a built-in" →
/// the row is excluded (custom/user-authored collections never auto-publish).
pub fn collection_hash(id: &str) -> Option<String> {
    use sha2::{Digest, Sha256};
    v2_json(id).map(|json| {
        let mut h = Sha256::new();
        h.update(json.as_bytes());
        format!("{:x}", h.finalize())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::eval::agentic::sandbox::{EndStateRule, TaskCheckpoint};
    use crate::inference::eval::agentic::v2::collection::load_v2_collection;
    use crate::inference::eval::agentic::v2::r#match::args_match_v2;
    use serde_json::Value;
    use std::collections::HashSet;

    fn has_wildcard(args: &Value) -> bool {
        args.as_object()
            .map(|o| o.values().any(|v| v.as_str().is_some_and(|s| s.contains('*'))))
            .unwrap_or(false)
    }

    /// `a` is a strict wildcard-superset of `b`: same tool, `a` has a wildcard, `a`'s
    /// pattern matches `b`'s args, but not the reverse — so greedy-first RequireAll
    /// matching could consume `a` with a call meant for the narrower `b` (false-negative).
    fn wildcard_superset(a: &TaskCheckpoint, b: &TaskCheckpoint) -> bool {
        a.tool == b.tool
            && has_wildcard(&a.args)
            && args_match_v2(&a.args, &b.args)
            && !args_match_v2(&b.args, &a.args)
    }

    /// Permanent answer-key guard: a future authored scenario can't silently regress
    /// these without failing the build.
    #[test]
    fn bundled_collections_pass_deep_integrity_checks() {
        for (id, json) in V2_SCENARIOS {
            for t in load_v2_collection(json).unwrap() {
                let spec = t.agentic.as_ref().unwrap();
                let tools: HashSet<&str> = t.tools.iter().map(|x| x.name.as_str()).collect();
                // name-keyed faults must name a presented tool (a typo'd on_call never fires).
                for nf in &spec.name_faults {
                    assert!(tools.contains(nf.on_call.as_str()), "{id}/{}: fault on_call '{}' not a tool", t.id, nf.on_call);
                }
                // no wildcard-superset shadowing among RequireAll checkpoints.
                if let EndStateRule::RequireAll(cps) = &spec.end_state {
                    for (i, a) in cps.iter().enumerate() {
                        for (j, b) in cps.iter().enumerate() {
                            assert!(i == j || !wildcard_superset(a, b), "{id}/{}: checkpoint {i} shadows {j}", t.id);
                        }
                    }
                }
                // field-scoped getters must name fields that exist in some world_state entity —
                // a typo'd field would silently surface `{}` (an honest-but-empty view that
                // teaches the model nothing), so reject it at load time, not at run time.
                if !spec.field_projections.is_empty() {
                    let mut known: HashSet<&str> = HashSet::new();
                    if let Some(ws) = spec.world_state.as_ref().and_then(|w| w.as_object()) {
                        for entity in ws.values() {
                            if let Some(obj) = entity.as_object() {
                                known.extend(obj.keys().map(String::as_str));
                            }
                        }
                    }
                    for (tool, fields) in &spec.field_projections {
                        for f in fields {
                            assert!(known.contains(f.as_str()), "{id}/{}: returns_fields '{f}' of '{tool}' names no world_state field", t.id);
                        }
                    }
                }
            }
        }
    }

    /// A glob literal segment containing an alnum_alnum snake_case token — a raw enum a
    /// natural-language reply will never contain verbatim.
    fn has_snake_token(seg: &str) -> bool {
        let c: Vec<char> = seg.chars().collect();
        c.windows(3).any(|w| w[0].is_alphanumeric() && w[1] == '_' && w[2].is_alphanumeric())
    }

    /// Customer-facing reply checkpoints must accept NATURAL phrasing: a text glob whose
    /// segment demands a raw snake_case token (`*in_transit*`) can never be satisfied by
    /// a truthful human reply ("your order is in transit"), so the run burns to the step
    /// cap and the verdict false-labels a correct model InfiniteLoop / FakeDone — the
    /// exact es_cs_check_order_status bug. Author them segmented (`*in*transit*`).
    /// Scoped to `reply_customer` (customer-facing prose): dev-facing reporters (e.g.
    /// coding's `reply`) legitimately require code identifiers echoed verbatim.
    #[test]
    fn customer_reply_globs_accept_natural_phrasing() {
        for (id, json) in V2_SCENARIOS {
            for t in load_v2_collection(json).unwrap() {
                let spec = t.agentic.as_ref().unwrap();
                let cps: Vec<&TaskCheckpoint> = match &spec.end_state {
                    EndStateRule::RequireAll(c) | EndStateRule::RequireSequence(c) => c.iter().collect(),
                    _ => Vec::new(),
                };
                for cp in cps.into_iter().filter(|cp| cp.tool == "reply_customer") {
                    let texts: Vec<&str> = match &cp.args {
                        Value::Object(o) => o.values().filter_map(|v| v.as_str()).collect(),
                        Value::String(s) => vec![s.as_str()],
                        _ => Vec::new(),
                    };
                    for pat in texts.into_iter().filter(|s| s.contains('*')) {
                        for seg in pat.split('*') {
                            assert!(
                                !has_snake_token(seg),
                                "{id}/{}: reply_customer glob '{pat}' demands raw token '{seg}' — a natural reply can't match; segment it (e.g. '*in*transit*')",
                                t.id
                            );
                        }
                    }
                }
            }
        }
    }

    /// A9 oracle gate: an agent that replays a task's expected_calls (substituting a
    /// wildcard-satisfying value for each `*…*` arg, and retrying through transient
    /// faults) must reach the end state on EVERY authored task — the per-collection
    /// answer-key / satisfiability proof. A no-call agent must fail (the floor).
    #[tokio::test]
    async fn an_oracle_satisfies_every_authored_task_and_a_trivial_agent_fails() {
        use crate::errors::AppResult;
        use crate::inference::eval::agentic::build::sandbox_for;
        use crate::inference::eval::agentic::model_turn::{ModelTurn, Progress};
        use crate::inference::eval::agentic::runner::run_once;
        use crate::inference::eval::agentic::spec::FaultInjection;
        use crate::inference::generate::generate_spec::GenerateSpec;
        use crate::inference::generate::generate_stats::GenerateStats;
        use serde_json::{json, Value};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::sync::mpsc::unbounded_channel;

        struct Scripted {
            calls: Vec<String>,
            next: AtomicUsize,
        }
        impl ModelTurn for Scripted {
            async fn run(&self, _s: &GenerateSpec, _progress: &Progress) -> AppResult<(String, GenerateStats)> {
                let i = self.next.fetch_add(1, Ordering::SeqCst);
                // Past the script: emit a no-op (no tool call) → never advances.
                let body = self.calls.get(i).cloned().unwrap_or_else(|| "{}".into());
                Ok((body, GenerateStats { eval_count: Some(1), ..Default::default() }))
            }
        }

        /// Replace each `*…*` string with a concrete value that satisfies the glob
        /// (its literal segments joined in order); keep everything else exact.
        fn concretize(v: &Value) -> Value {
            match v {
                Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), concretize(x))).collect()),
                Value::String(s) if s.contains('*') => {
                    let lit: String = s.split('*').filter(|p| !p.is_empty()).collect();
                    Value::String(if lit.is_empty() { "x".into() } else { lit })
                }
                other => other.clone(),
            }
        }

        for (id, json_str) in V2_SCENARIOS {
            for t in load_v2_collection(json_str).unwrap() {
                let spec = t.agentic.as_ref().unwrap();
                // Build the oracle's call script: each checkpoint, repeated enough to
                // clear a transient fault on its tool (fault fires before the advance).
                let mut calls = Vec::new();
                // A transient fault is keyed by tool NAME (global counter), so the
                // oracle only needs the extra retries on the tool's FIRST occurrence.
                let mut cleared: std::collections::HashSet<String> = std::collections::HashSet::new();
                if let EndStateRule::RequireAll(cps) = &spec.end_state {
                    for cp in cps {
                        let retries = if cleared.insert(cp.tool.clone()) {
                            spec.name_faults
                                .iter()
                                .find(|f| f.on_call == cp.tool)
                                .map(|f| match f.fault {
                                    FaultInjection::TransientError { clears_after, .. } => clears_after as usize,
                                    FaultInjection::PersistentError { .. } => 0,
                                })
                                .unwrap_or(0)
                        } else {
                            0
                        };
                        let body = json!({ "name": cp.tool, "args": concretize(&cp.args) }).to_string();
                        for _ in 0..=retries {
                            calls.push(body.clone());
                        }
                    }
                }
                // RequireEndState (stateful web-UI) is graded on the final state, not checkpoints,
                // so script the oracle from the authored `expected_calls` (the UI actions that
                // drive the state to the target) read from the raw JSON.
                if matches!(spec.end_state, EndStateRule::RequireEndState(_)) {
                    let raw: Value = serde_json::from_str(json_str).unwrap();
                    let task_json = raw["tasks"].as_array().unwrap().iter().find(|x| x["id"] == json!(t.id)).unwrap();
                    for ec in task_json["expected_calls"].as_array().into_iter().flatten() {
                        calls.push(json!({ "name": ec["name"], "args": concretize(&ec["args"]) }).to_string());
                    }
                }
                let (sandbox, cfg) = sandbox_for(&t).unwrap();

                // Oracle-perfect run → reaches the end state, no decoys, no traps.
                let oracle = Scripted { calls, next: AtomicUsize::new(0) };
                let (tx, _rx) = unbounded_channel();
                let ok = run_once(&oracle, &sandbox, cfg.max_steps, cfg.max_recovery, 0, &tx).await.unwrap();
                assert!(ok.reached_end, "{id}/{}: oracle did not reach end state", t.id);
                assert_eq!(ok.unknown_tool_calls, 0, "{id}/{}: oracle hit an unknown tool", t.id);
                assert_eq!(ok.failure, None, "{id}/{}: oracle failed ({:?})", t.id, ok.failure);

                // Trivial floor: a no-call agent never satisfies a RequireAll / RequireEndState task.
                if matches!(spec.end_state, EndStateRule::RequireAll(_) | EndStateRule::RequireEndState(_)) {
                    let lazy = Scripted { calls: vec![], next: AtomicUsize::new(0) };
                    let (tx2, _r2) = unbounded_channel();
                    let bad = run_once(&lazy, &sandbox, cfg.max_steps, cfg.max_recovery, 0, &tx2).await.unwrap();
                    assert!(!bad.reached_end, "{id}/{}: a trivial agent must NOT pass", t.id);
                }
            }
        }
    }

    /// LIVE (ignored): field-scoped getters against a real Ollama model. Runs the API-key
    /// rotation task and dumps every call + injected tool result, so we can eyeball that
    /// `get_service` surfaces ONLY `class` (no `active_sessions` leak) while `check_sessions`
    /// surfaces `active_sessions` — the whole point of the field-projection change (cp3 is now
    /// load-bearing, not redundant). Run:
    ///   cargo test --lib live_field_scoped_rotation -- --ignored --nocapture
    #[tokio::test]
    #[ignore = "hits a live Ollama on :11434 with qwen2.5-coder-7b-instruct installed"]
    async fn live_field_scoped_rotation_splits_class_from_sessions() {
        use crate::inference::backend::backend_kind::BackendKind;
        use crate::inference::eval::agentic::build::sandbox_for;
        use crate::inference::eval::agentic::model_turn::{BackendTurn, ModelTurn};
        use crate::inference::eval::agentic::runner::{run_once, NUM_CTX_CEILING};
        use tokio::sync::mpsc::unbounded_channel;
        use tokio_util::sync::CancellationToken;

        let model = std::env::var("QM_LIVE_MODEL").unwrap_or_else(|_| "qwen2.5-coder-7b-instruct:q4_k_m".into());
        let tasks = load_v2_collection(V2_SCENARIOS.iter().find(|(id, _)| *id == "medium-coding").unwrap().1).unwrap();
        let t = tasks.into_iter().find(|t| t.id == "md_co_secret_rotation_by_svc").expect("task present");
        let (sandbox, cfg) = sandbox_for(&t).unwrap();

        // Prompt path (JSON dialect) — the same path the reported failing run used; no native
        // tool support needed, so it runs on any chat model.
        let turn = BackendTurn {
            backend: BackendKind::Ollama,
            endpoint: "http://localhost:11434".into(),
            model,
            cancel: CancellationToken::new(),
            options: None,
            keep_alive: Some(300),
            is_thinking: false,
            max_tokens: 512,
            cpu_offloaded: false,
            ctx_ceiling: NUM_CTX_CEILING,
            stop_cache: Default::default(),
        };
        turn.warm_up().await.unwrap();
        let (tx, mut rx) = unbounded_channel();
        let outcome = run_once(&turn, &sandbox, cfg.max_steps, cfg.max_recovery, 0, &tx).await.unwrap();
        drop(tx);

        let mut get_service_injections = Vec::new();
        let mut check_sessions_injections = Vec::new();
        eprintln!("\n===== LIVE trajectory: md_co_secret_rotation_by_svc =====");
        while let Ok(step) = rx.try_recv() {
            eprintln!("[step {}] kind={:?}\n  CALL: {}\n  RESULT: {:?}", step.step_index, step.kind, step.raw_output.trim(), step.injection);
            if let Some(inj) = &step.injection {
                if step.raw_output.contains("get_service") { get_service_injections.push(inj.clone()); }
                if step.raw_output.contains("check_sessions") { check_sessions_injections.push(inj.clone()); }
            }
        }
        eprintln!("VERDICT: reached_end={} failure={:?}", outcome.reached_end, outcome.failure);

        // Data-quality gate — the DISJOINTNESS invariant, true for ANY trajectory (independent of
        // model capability or which call trips the 503 fault): the two endpoints never overlap.
        // get_service never surfaces active_sessions; check_sessions never surfaces class. This is
        // exactly the redundancy the projection removed — one endpoint can't substitute for the other.
        for inj in &get_service_injections {
            assert!(!inj.contains("active_sessions"), "get_service leaked active_sessions: {inj}");
        }
        for inj in &check_sessions_injections {
            assert!(!inj.contains("class"), "check_sessions leaked class (whole blob): {inj}");
        }
        // The positive (check_sessions{S-2} → {"active_sessions":true}, get_service → {"class":…}) is
        // pinned deterministically in the sandbox/world_state unit tests + the oracle satisfiability
        // gate; here we log whichever the live model actually elicited.
        eprintln!(
            "get_service results seen: {get_service_injections:?}\ncheck_sessions results seen: {check_sessions_injections:?}"
        );
    }

    /// Every nested STRING value in `v` (object values + array elements; object KEYS
    /// are not values, so they're excluded — a key is never a "discovered fact").
    fn string_values(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::String(s) => out.push(s.clone()),
            Value::Array(a) => a.iter().for_each(|x| string_values(x, out)),
            Value::Object(o) => o.values().for_each(|x| string_values(x, out)),
            _ => {}
        }
    }

    /// Replace each `*…*` glob string with its literal segments joined, so a getter's
    /// wildcard discovery arg resolves to a concrete value the responder can key on.
    fn concretize_args(v: &Value) -> Value {
        match v {
            Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), concretize_args(x))).collect()),
            Value::String(s) if s.contains('*') => {
                let lit: String = s.split('*').filter(|p| !p.is_empty()).collect();
                Value::String(if lit.is_empty() { "x".into() } else { lit })
            }
            other => other.clone(),
        }
    }

    /// Answer-key REACHABILITY guard (the inverse of "every entity arg resolves to a
    /// key"): every discovered-only world_state fact a checkpoint forces the model to
    /// echo must be retrievable through SOME getter the model can call. A fact in
    /// world_state but unreachable through any tool (the `es_co_run_failing_test` bug:
    /// the failing test name lived under key `cart_tests`, but `run_tests{module:"cart"}`
    /// resolved nothing) makes the task unsolvable by a real model — only the oracle's
    /// replay "passes" it. Respects `returns_entity`: a getter mistagged as an action
    /// stops surfacing its fact, so this also guards the tags.
    ///
    /// SCOPE: ALL tiers, entity env only — the fs/corpus/web-ui responders don't derive
    /// from `derive_response`, so the surfacing simulation below doesn't model them.
    /// Originally Easy-only: Medium+ was deferred because reasoned conclusions were
    /// thought to trip the string-needle heuristic. In practice the needle only fires
    /// when a checkpoint arg segment contains a COMPLETE ws string value — a literal
    /// echo, i.e. a retrieval demand; reasoned conclusions are either short tokens
    /// inside long oracle strings (never contained) or short ws values that the
    /// grounding pass made surfaceable through a checkpointed getter. With every
    /// retrieved fact now resolvable (the answer-key grounding pass), the guard holds
    /// tier-wide — closing the follow-up recorded in
    /// `docs/process.md#future-considerations`.
    #[test]
    fn every_required_world_state_fact_is_tool_reachable_all_tiers() {
        use crate::inference::eval::agentic::v2::world_state::derive_response;
        use crate::inference::eval::toolcall::tasks::Call;

        let mut violations: Vec<String> = Vec::new();
        for (id, json) in V2_SCENARIOS {
            let v: Value = serde_json::from_str(json).unwrap();
            if matches!(v.get("environment").and_then(Value::as_str), Some("filesystem") | Some("web_corpus") | Some("web_ui")) {
                continue; // non-entity responders — derive_response doesn't model them
            }
            for task in v["tasks"].as_array().into_iter().flatten() {
                let tid = task["id"].as_str().unwrap_or("?");
                let ws = &task["world_state"];
                if ws.is_null() {
                    continue;
                }
                let prompt = task["prompt"].as_str().unwrap_or("").to_lowercase();
                // Getter set: real tools whose `returns_entity` isn't false.
                let getters: HashSet<&str> = task["tools"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|t| t["returns_entity"].as_bool() != Some(false))
                    .filter_map(|t| t["name"].as_str())
                    .collect();
                let calls: Vec<&Value> =
                    task["expected_calls"].as_array().into_iter().flatten().filter(|e| e["type"] == "call").collect();
                // What the model can SURFACE: derive_response over every getter call.
                let mut surfaced = String::new();
                for ec in &calls {
                    let name = ec["name"].as_str().unwrap_or("");
                    if !getters.contains(name) {
                        continue;
                    }
                    let call = Call { name: name.into(), args: concretize_args(&ec["args"]) };
                    surfaced.push_str(&derive_response(ws, &call));
                    surfaced.push('\n');
                }
                // Discovered-only facts: ws string values not already in the prompt.
                let mut ws_vals = Vec::new();
                string_values(ws, &mut ws_vals);
                // Each checkpoint arg literal that DEMANDS a discovered fact must be reachable.
                for ec in &calls {
                    let mut arg_strs = Vec::new();
                    string_values(&ec["args"], &mut arg_strs);
                    for s in &arg_strs {
                        for seg in s.split('*').filter(|p| !p.is_empty()) {
                            for wv in &ws_vals {
                                if wv.len() >= 4
                                    && !prompt.contains(&wv.to_lowercase())
                                    && seg.contains(wv.as_str())
                                    && !surfaced.contains(wv.as_str())
                                {
                                    violations.push(format!("{id}/{tid}: fact '{wv}' is required by a checkpoint but unreachable by any getter"));
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(violations.is_empty(), "unreachable answer-key facts:\n{}", violations.join("\n"));
    }

    /// Tag guard (all tiers): a retrieval-shaped tool must never be tagged as an action.
    /// A getter mistagged `returns_entity:false` would ack instead of surfacing data, so
    /// a real model could never retrieve the fact (the exact harness-fails-correct-model
    /// bug class). This catches such a mistag structurally — without the false positives
    /// a reasoned-vs-retrieved heuristic carries — so action-tagging stays safe to extend.
    #[test]
    fn no_retrieval_shaped_tool_is_tagged_as_an_action() {
        const GETTER_PREFIX: [&str; 16] = [
            "get_", "check_", "compute_", "run_", "read_", "search_", "classify_", "verify_",
            "validate_", "screen_", "scan_", "assess_", "identify_", "test_", "fit_", "convert_",
        ];
        const GETTER_EXACT: [&str; 6] =
            ["calc", "chem_lookup", "blast_radius", "mark_to_market", "impute_missing", "format_value"];
        let mut violations: Vec<String> = Vec::new();
        for (id, json) in V2_SCENARIOS {
            let v: Value = serde_json::from_str(json).unwrap();
            for task in v["tasks"].as_array().into_iter().flatten() {
                let tid = task["id"].as_str().unwrap_or("?");
                for tool in task["tools"].as_array().into_iter().flatten() {
                    let name = tool["name"].as_str().unwrap_or("");
                    let getter_shaped =
                        GETTER_PREFIX.iter().any(|p| name.starts_with(p)) || GETTER_EXACT.contains(&name);
                    if getter_shaped && tool["returns_entity"].as_bool() == Some(false) {
                        violations.push(format!("{id}/{tid}: getter-shaped tool '{name}' tagged returns_entity:false"));
                    }
                }
            }
        }
        assert!(violations.is_empty(), "retrieval getters mistagged as actions:\n{}", violations.join("\n"));
    }

    #[test]
    fn collection_hash_is_stable_for_builtins_and_none_for_custom() {
        // Deterministic 64-char lowercase hex, identical across calls.
        let a = collection_hash("easy-coding").expect("built-in must hash");
        let b = collection_hash("easy-coding").expect("built-in must hash");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        // Distinct collections hash differently.
        assert_ne!(a, collection_hash("hard-coding").unwrap());
        // Unknown / custom ids are not built-ins → None (drives the exclusion gate).
        assert_eq!(collection_hash("my-custom-collection"), None);
        assert_eq!(collection_hash(""), None);
    }

    #[test]
    fn every_bundled_v2_collection_loads_and_validates() {
        assert_eq!(V2_SCENARIOS.len(), 27);
        for (id, json) in V2_SCENARIOS {
            let tasks = load_v2_collection(json).unwrap_or_else(|e| panic!("collection '{id}' failed to load: {e}"));
            assert!(!tasks.is_empty(), "collection '{id}' has no tasks");
            // Every bundled task routes through the agentic engine.
            assert!(tasks.iter().all(|t| t.category == "agent_loop"), "collection '{id}' must be all agent_loop");
            // The header parses (domain + tier for the picker).
            let h = v2_header(json).unwrap_or_else(|| panic!("collection '{id}' header unparseable"));
            assert!(!h.domain.is_empty() && !h.tier.is_empty(), "collection '{id}' missing domain/tier");
        }
    }

    /// LOADED (not mirrored) action-ack guard. Builds the REAL `es_co_branch_target` sandbox
    /// from the bundled JSON through the actual transpile path, and proves `open_pr` (an
    /// action) acks `{"ok":true}` — it never echoes the `{"kind":...}` entity. A unit test
    /// that hand-mirrors the task can't catch a per-occurrence mistag in the bundled file;
    /// this loads the file. (The trace showing `open_pr` echo was a pre-`851f5cd` binary —
    /// multi-call had landed, action-ack hadn't; this pins that current source+transpile ack.)
    #[test]
    fn loaded_branch_target_acks_open_pr_and_echoes_get_change() {
        use crate::inference::eval::agentic::build::sandbox_for;
        use crate::inference::eval::toolcall::tasks::Call;
        use serde_json::json;
        let json = v2_json("easy-coding").unwrap();
        let task = load_v2_collection(json).unwrap().into_iter().find(|t| t.id == "es_co_branch_target").unwrap();
        let (sandbox, _) = sandbox_for(&task).unwrap();
        // open_pr is an ACTION: excluded from the getter set, and a real entity-keyed call acks.
        assert!(!sandbox.entity_tools.contains("open_pr"), "open_pr leaked into the getter set");
        assert_eq!(
            sandbox.respond(&Call { name: "open_pr".into(), args: json!({ "change": "C-1", "base": "release" }) }).as_deref(),
            Some(r#"{"ok":true}"#),
        );
        // get_change is a GETTER: surfaces the entity, so the kind stays reachable (split is real).
        assert!(sandbox.entity_tools.contains("get_change"));
        assert_eq!(
            sandbox.respond(&Call { name: "get_change".into(), args: json!({ "id": "C-1" }) }).as_deref(),
            Some(r#"{"kind":"hotfix"}"#),
        );
    }

    /// `reply_tool_name` invariant across EVERY bundled task — the backstop for the
    /// "first tool with a `text` property" heuristic the act-task prompt mandate relies on:
    /// (1) every task has AT MOST ONE text-bearing tool, so "first" is never order-ambiguous;
    /// (2) an ACT task with a reporter checkpoint (a `text` arg) resolves `reply_tool_name`
    /// to exactly that tool (so `MustUseTools` names the REAL reporter — `reply` vs
    /// `reply_customer`); (3) an action-only ACT task resolves to `None` (so the mandate never
    /// points at a `reply` tool that doesn't exist — the phantom-call foot-gun). A future
    /// text-bearing action tool or a second reporter fails THIS test at CI, not a live trace.
    #[test]
    fn reply_tool_name_classifies_every_task_and_reporters_are_unique() {
        use crate::inference::eval::agentic::sandbox::EndStateRule;
        use crate::inference::eval::toolcall::prompt::reply_tool_name;
        for (id, json) in V2_SCENARIOS {
            for t in load_v2_collection(json).unwrap() {
                let text_tools = t
                    .tools
                    .iter()
                    .filter(|x| x.parameters.get("properties").and_then(|p| p.get("text")).is_some())
                    .count();
                assert!(text_tools <= 1, "{id}/{}: {text_tools} text-bearing tools — reply_tool_name 'first' is ambiguous", t.id);
                // Only ACT tasks consult reply_tool_name (abstain uses PlainTextOk).
                if let EndStateRule::RequireAll(cps) | EndStateRule::RequireSequence(cps) = &t.agentic.as_ref().unwrap().end_state {
                    let reporter = cps.iter().find(|c| c.args.get("text").is_some()).map(|c| c.tool.as_str());
                    match reporter {
                        Some(tool) => assert_eq!(reply_tool_name(&t.tools), Some(tool), "{id}/{}: reporter checkpoint tool must be the detected reply tool", t.id),
                        None => assert_eq!(reply_tool_name(&t.tools), None, "{id}/{}: action-only ACT task must resolve to NO reply tool (else MustUseTools names a phantom)", t.id),
                    }
                }
            }
        }
    }

    /// Generalized per-OCCURRENCE tag-threading guard across EVERY bundled task. Loads each
    /// task through the real transpile and asserts each tool's `returns_entity` tag threads
    /// into `entity_tools` per occurrence: an action (`false`) must NOT be a getter (else it
    /// echoes entity data — leaking a discovery target like `es_cs_lang_routing`'s `pref`);
    /// a getter must BE one (else it acks and hides its required fact). Catches a mistag on
    /// any single task's tool that a correct tag elsewhere would otherwise mask — the
    /// per-occurrence hole a task-mirroring test cannot see.
    #[test]
    fn every_action_tool_threads_to_ack_in_the_real_sandbox() {
        use crate::inference::eval::agentic::build::sandbox_for;
        for (id, json) in V2_SCENARIOS {
            let raw: Value = serde_json::from_str(json).unwrap();
            // `entity_tools` getter/action threading is entity-mode semantics. The FileSystem,
            // WebCorpus, and (stateful) WebUi responders dispatch actions by name + mutate/return
            // real content (`entity_tools` is unused), so this invariant doesn't apply to them.
            if matches!(raw.get("environment").and_then(Value::as_str), Some("filesystem") | Some("web_corpus") | Some("web_ui")) {
                continue;
            }
            let tasks = load_v2_collection(json).unwrap();
            for rawtask in raw["tasks"].as_array().into_iter().flatten() {
                let tid = rawtask["id"].as_str().unwrap_or("?");
                let task = tasks.iter().find(|t| t.id == tid).unwrap();
                let (sandbox, _) = sandbox_for(task).unwrap();
                for tool in rawtask["tools"].as_array().into_iter().flatten() {
                    let name = tool["name"].as_str().unwrap_or("");
                    if tool["returns_entity"].as_bool() == Some(false) {
                        assert!(!sandbox.entity_tools.contains(name), "{id}/{tid}: action '{name}' is in the getter set — would echo entity data (leak)");
                    } else {
                        assert!(sandbox.entity_tools.contains(name), "{id}/{tid}: getter '{name}' missing from the getter set — would ack and hide its fact");
                    }
                }
            }
        }
    }

    /// LOADED (not mirrored) grounding probe for the returns fix: the REAL transpiled
    /// hd_se_returns sandbox must surface the marketplace policy and state e-waste rule
    /// — the two getters the live trace showed acking. Loads through the actual
    /// collection→transpile→sandbox_for path, so a transpile-layer regression (world_state
    /// filtered, entity_tools mis-threaded) fails HERE without needing a live model.
    #[test]
    fn loaded_returns_task_surfaces_policy_and_ewaste_facts() {
        use crate::inference::eval::agentic::build::sandbox_for;
        use crate::inference::eval::toolcall::tasks::Call;
        use serde_json::json;
        let task = load_v2_collection(v2_json("hard-support-ecommerce").unwrap())
            .unwrap()
            .into_iter()
            .find(|t| t.id == "hd_se_returns_instance0")
            .unwrap();
        let (sandbox, _) = sandbox_for(&task).unwrap();
        let policy = sandbox.respond(&Call { name: "get_marketplace_policy".into(), args: json!({ "mkt": "MShop" }) });
        assert!(
            policy.as_deref().is_some_and(|r| r.contains("restocking")),
            "get_marketplace_policy(MShop) must surface the policy text, got {policy:?}"
        );
        let ewaste = sandbox.respond(&Call { name: "get_state_ewaste_rule".into(), args: json!({ "state": "SD" }) });
        assert!(
            ewaste.as_deref().is_some_and(|r| r.contains("e-waste")),
            "get_state_ewaste_rule(SD) must surface the rule, got {ewaste:?}"
        );
    }

    /// A REALISTIC test-file path must serve the test source, not `not found` —
    /// the data/checkpoint asymmetry the trace audit surfaced: the checkpoint glob
    /// `*test_round_paise*` advanced on `tests/test_round_paise.py` while the
    /// responder only resolved the bare `test_round_paise` key. The alias key +
    /// the `failing_test_file` field in the run_tests blob close it: the model
    /// learns the real path AND fetching it returns the source (whose comment
    /// grounds the required quantize fix).
    #[test]
    fn loaded_trace_root_cause_serves_the_test_source_for_a_realistic_path() {
        use crate::inference::eval::agentic::build::sandbox_for;
        use crate::inference::eval::toolcall::tasks::Call;
        use serde_json::json;
        let task = load_v2_collection(v2_json("medium-coding").unwrap())
            .unwrap()
            .into_iter()
            .find(|t| t.id == "md_co_trace_root_cause")
            .unwrap();
        let (sandbox, _) = sandbox_for(&task).unwrap();
        // run_tests names the real file, the way a test runner reports…
        let run = sandbox.respond(&Call { name: "run_tests".into(), args: json!({ "module": "payments" }) });
        assert!(
            run.as_deref().is_some_and(|r| r.contains("tests/test_round_paise.py")),
            "run_tests must surface the failing test FILE, got {run:?}"
        );
        // …and reading that realistic path returns the test source (with the
        // quantize expectation), same as the bare test-name key.
        for path in ["tests/test_round_paise.py", "test_round_paise"] {
            let src = sandbox.respond(&Call { name: "read_file".into(), args: json!({ "path": path }) });
            assert!(
                src.as_deref().is_some_and(|r| r.contains("quantize")),
                "read_file({path}) must serve the test source, got {src:?}"
            );
        }
    }

    /// Every bundled collection, loaded through the REAL transpile path, must be
    /// clean under `oracle::semantic_findings` filtered to `kind` — the same
    /// implementation `evals::save` hard-blocks custom collections on, so the CI
    /// contract and the import trust boundary can never drift.
    fn assert_bundled_clean_for(kind: crate::inference::eval::agentic::v2::oracle::SemanticFindingKind) {
        use crate::inference::eval::agentic::v2::collection::load_v2_collection;
        use crate::inference::eval::agentic::v2::oracle::semantic_findings;

        let mut violations: Vec<String> = Vec::new();
        for (id, json) in V2_SCENARIOS {
            let tasks = load_v2_collection(json).unwrap();
            for f in semantic_findings(&tasks) {
                if f.kind == kind {
                    violations.push(format!("{id}/{f}"));
                }
            }
        }
        assert!(violations.is_empty(), "world-state authoring contract violated ({kind:?}):\n{}", violations.join("\n"));
    }

    /// All-tier answer-key DATA guard (entity env): every expected getter call must
    /// resolve to real `world_state` data — never the generic `{"ok":true}` ack. A
    /// getter that acks hides the fact the task expects the model to discover (the
    /// hard-support-ecommerce bug: `get_marketplace_policy{mkt:"MShop"}` acked because
    /// the policy text lived NESTED under `ws["policy"]`, unreachable by
    /// `derive_response`'s top-level key lookup) — the model then decides on missing
    /// data and springs the trap through no fault of its own. Reporter tools (the
    /// `text`-bearing reply channel) are exempt: their ack IS the response. The
    /// filesystem/corpus/web-ui environments don't use the entity responder. A raw
    /// arg is tried before its glob-concretized form so a `calc` expression's literal
    /// `*` (multiplication) is never mangled into a false violation.
    #[test]
    fn every_expected_getter_call_resolves_to_real_world_state_data() {
        assert_bundled_clean_for(crate::inference::eval::agentic::v2::oracle::SemanticFindingKind::AckingGetter);
    }

    /// Entity-discoverability guard (mirrors `generator::instantiate`): every
    /// digit-bearing, non-`RESERVED` top-level `world_state` key must be reachable by
    /// the model — named as a whole-word token in the task prompt (a ROOT entity the
    /// model is told to act on), OR referenced inside another top-level entry's
    /// serialized value (a DISCOVERED entity surfaced by some getter's blob, e.g. a
    /// wire blob naming its counterparty `CP-CLEAN-1`). Two failures share this root
    /// cause: (1) `instantiate()` alpha-renames these ids across the prompt AND
    /// world_state via `replace_ids` — an id reachable through neither surface anchors
    /// nothing; (2) more fundamentally, a model handed no path to an id can't know
    /// which entity to fetch, so it asks a clarifying question — a no-tool-call turn
    /// the runner scores as `HallucinatedCompletion` (every run fails identically). A
    /// future scenario that ships an orphaned entity id fails HERE, at CI, not in a
    /// live trace.
    #[test]
    fn every_world_state_entity_id_is_reachable_from_the_prompt_or_a_blob() {
        assert_bundled_clean_for(crate::inference::eval::agentic::v2::oracle::SemanticFindingKind::OrphanEntity);
    }

    /// Answer-key-leak guard, the inverse of the reachability guard: every
    /// non-`RESERVED` top-level `world_state` key must be INTENDED-fetchable —
    /// named whole-word in the prompt, referenced in another entity's blob,
    /// an expected-call arg value, or a tool name (the no-arg fallback). A key
    /// reachable through none of those is pure oracle data (`outcome`, `rule`,
    /// `real_bug`, …) that no correct play ever fetches — yet `derive_response`
    /// would hand back its whole blob to any call whose arg happens to equal the
    /// key string. Such a key must be listed in `world_state::RESERVED` (which
    /// makes the responder ack instead). This guard keeps RESERVED exact in both
    /// directions: an unfetchable key missing from RESERVED fails here; a key
    /// added to RESERVED that a real getter needed flips
    /// `every_expected_getter_call_resolves_to_real_world_state_data` red.
    #[test]
    fn no_unfetched_world_state_key_is_resolvable_by_a_getter() {
        assert_bundled_clean_for(crate::inference::eval::agentic::v2::oracle::SemanticFindingKind::UnfetchedKey);
    }

    /// Answer-grounding guard: every glob literal an expected action/reporter
    /// checkpoint demands must be teachable — in the prompt, a tool name, or data
    /// an earlier expected call surfaces. Without this, a checkpoint like
    /// `decision:"*file non-suspensory*"` grades on vocabulary the model has no
    /// way to read (the full-suite step-response audit found six such tasks) — a
    /// capable model phrases the same conclusion in its own words and is scored a
    /// false-negative FAIL. Fix direction is always to GROUND the wording in the
    /// blob the intended play reads, never to loosen the checkpoint.
    #[test]
    fn every_expected_answer_token_is_grounded_in_reachable_data() {
        assert_bundled_clean_for(crate::inference::eval::agentic::v2::oracle::SemanticFindingKind::UngroundedAnswerToken);
    }
}
