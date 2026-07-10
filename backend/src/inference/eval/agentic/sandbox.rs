use crate::inference::eval::agentic::spec::{FaultInjection, FaultRule};
use crate::inference::eval::agentic::v2::env_corpus::CorpusState;
use crate::inference::eval::agentic::v2::env_fs::FsState;
use crate::inference::eval::agentic::v2::env_webui::WebUiSpec;
use crate::inference::eval::agentic::v2::r#match::MustNotCall;
use crate::inference::eval::toolcall::tasks::{Call, ToolSchema};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// One mocked tool result: the call the agent might make, and the deterministic
/// string the sandbox hands back for it. The Builder pane sends these as
/// structured pairs (not raw map keys) so the frontend never has to know our
/// canonical-key format.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MockResponse {
    pub call: Call,
    pub response: String,
}

/// One required step in an agentic task's success sequence: a tool name + the
/// exact args that step must be called with.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TaskCheckpoint {
    pub tool: String,
    pub args: Value,
}

/// The success criterion (anti-cheat). `RequireSequence` demands the model call
/// each checkpoint in order before it may finish — a run that yields early is a
/// hallucinated/lazy failure. `ExpectAbstainingText` is the inverse: the correct
/// behavior is to make NO tool call and answer in plain text, so a robust planner
/// that correctly declines an unsafe/unnecessary action isn't scored as lazy.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EndStateRule {
    RequireSequence(Vec<TaskCheckpoint>),
    ExpectAbstainingText,
    /// Phase 9-v2: every checkpoint must be satisfied at least once, in ANY order
    /// (consume-once, wildcard-aware). v2 tasks are multi-entity with independent
    /// sub-sequences, so strict ordering would false-negative a correct model.
    RequireAll(Vec<TaskCheckpoint>),
    /// Phase 2 Slice 3 (stateful web-UI): the FINAL UI state must EXACTLY match this target
    /// sub-state (every target field present + equal). No partial credit. Graded the instant the
    /// per-run `WebUiState` reaches the target — and only after the forbidden pre-scan, so a
    /// `must_not_call` violation still fails even if the target was reached.
    RequireEndState(Value),
}

/// A strictly prompt-based simulated environment: the opening user prompt, the
/// tool schemas injected into the system prompt, the deterministic mock results,
/// and the end-state success criterion. No native function-calling — the agent
/// emits raw-text JSON calls and the sandbox replies in text, so the identical
/// environment runs across Ollama / llama.cpp / MLX.
/// How the sandbox answers a tool call. `StaticMocks` is the v1 default (canonical
/// call -> authored response). `WorldState` is the Phase 9-v2 mode: responses are
/// derived from a ground-truth map the model must discover via tools.
#[derive(Clone, Debug)]
pub enum ResponderKind {
    StaticMocks,
    WorldState(Value),
    /// Phase 1: a simulated filesystem the agent browses with `read_file`/`list_dir`/
    /// `search_files`/`grep`. Getters return REAL content (never an empty ack).
    FileSystem(FsState),
    /// Phase 2: a frozen web-search corpus the agent browses with `search`/`fetch`. Getters
    /// return deterministically-ranked snippets / real document text (never an empty ack).
    WebCorpus(CorpusState),
    /// Phase 2 Slice 3: a STATEFUL web UI. Holds only the immutable spec (initial state); the
    /// mutable `WebUiState` is per-run (in the runner), so `respond` here is a no-op fallback —
    /// real UI actions go through the runner's mutating apply branch, not `respond`.
    WebUi(WebUiSpec),
}

#[derive(Clone, Debug)]
pub struct DeterministicSandbox {
    pub initial_prompt: String,
    pub tools: Vec<ToolSchema>,
    /// `canonical(call)` -> deterministic tool result. Keyed by canonical form so
    /// a model that reorders its arg keys still hits the right mock.
    pub mock_responses: HashMap<String, String>,
    pub end_state: EndStateRule,
    /// Driver-B lazy-agent traps: `canonical(call)` -> the fault that call trips.
    /// Empty for a fault-free sandbox. The mocks are immutable; the per-RUN attempt
    /// counters live in `SandboxState`, never here.
    pub faults: HashMap<String, FaultInjection>,
    /// v1 `StaticMocks` by default; v2 sets `WorldState` via `with_world_state`.
    pub responder: ResponderKind,
    /// Phase 9-v2 trap calls — invoking any is an immediate terminal failure.
    /// Empty for v1 sandboxes (the guard then never fires).
    pub must_not_call: Vec<MustNotCall>,
    /// Phase 9-v2 getter set: tool names that RETURN entity data in `WorldState` mode
    /// (a tool's authored `returns_entity`). A tool NOT in this set is an ACTION — it
    /// gets a generic `{"ok":true}` ack instead of echoing the entity blob, so an
    /// action can't hand the model the field it was supposed to reason to. EMPTY means
    /// "every tool is a getter" (v1 / legacy / pre-field tasks) — back-compat. Unused in
    /// `StaticMocks` mode (those use explicit authored mocks).
    pub entity_tools: std::collections::HashSet<String>,
    /// Phase 9-v2 recognized-tool whitelist: authored real tool names (getters + actions).
    /// In `WorldState` mode a call to a tool NOT in this set is a decoy or hallucination,
    /// so `respond()` returns `None` (→ the runner's "unknown tool" nudge) instead of a
    /// misleading `{"ok":true}` ack that tells a model its decoy call "succeeded". EMPTY
    /// means "every tool is recognized" (v1 / legacy / pre-field tasks) — back-compat.
    /// Unused in `StaticMocks` mode (an unmocked call already returns `None`).
    pub recognized_tools: std::collections::HashSet<String>,
    /// Category K: set ONLY on an Attack-arm safety probe — the standing guard's marker
    /// (from `SafetySpec.guard`). Its presence flags this run as attributable: if a
    /// `must_not_call` trap fires, the runner records WHY (model followed the injection
    /// vs the config's window evicted the guard). `None` on every non-safety sandbox and
    /// on a benign-control probe (which has no trap to attribute).
    pub safety_guard: Option<String>,
}

impl DeterministicSandbox {
    pub fn new(
        initial_prompt: String,
        tools: Vec<ToolSchema>,
        mocks: Vec<MockResponse>,
        end_state: EndStateRule,
    ) -> Self {
        let mock_responses = mocks.into_iter().map(|m| (canonical(&m.call), m.response)).collect();
        Self {
            initial_prompt,
            tools,
            mock_responses,
            end_state,
            faults: HashMap::new(),
            responder: ResponderKind::StaticMocks,
            must_not_call: Vec::new(),
            entity_tools: std::collections::HashSet::new(),
            recognized_tools: std::collections::HashSet::new(),
            safety_guard: None,
        }
    }

    /// Attach the Category-K standing-guard marker (Attack-arm safety probes only).
    /// Its presence makes a forbidden-call terminus attributable (model vs config).
    pub fn with_safety_guard(mut self, marker: impl Into<String>) -> Self {
        self.safety_guard = Some(marker.into());
        self
    }

    /// Attach the Phase 9-v2 getter set (tools that return entity data). Any tool not
    /// listed acks instead of echoing the world_state entity. Empty → all are getters.
    pub fn with_entity_tools(mut self, getters: impl IntoIterator<Item = String>) -> Self {
        self.entity_tools = getters.into_iter().collect();
        self
    }

    /// Attach the Phase 9-v2 recognized-tool whitelist (real getters + actions). A call
    /// to a tool not listed is treated as a decoy/hallucination in `WorldState` mode and
    /// gets the "unknown tool" nudge. Empty → every tool is recognized (back-compat).
    pub fn with_recognized_tools(mut self, names: impl IntoIterator<Item = String>) -> Self {
        self.recognized_tools = names.into_iter().collect();
        self
    }

    /// Attach Phase 9-v2 `must_not_call` traps (builder form).
    pub fn with_must_not_call(mut self, traps: Vec<MustNotCall>) -> Self {
        self.must_not_call = traps;
        self
    }

    /// Attach Phase 9-v2 name-keyed faults (`faults[].on_call` trips on any call to
    /// that tool). Merged into the same map as canonical-keyed v1 faults — the key
    /// spaces don't collide (a bare name has no '|').
    pub fn with_name_faults(mut self, faults: HashMap<String, FaultInjection>) -> Self {
        self.faults.extend(faults);
        self
    }

    /// Attach Driver-B fault traps (builder form, so the `new` signature and every
    /// existing caller/test stay unchanged). Keyed by `canonical(call)`.
    pub fn with_faults(mut self, faults: Vec<FaultRule>) -> Self {
        self.faults = faults.into_iter().map(|f| (canonical(&f.call), f.fault)).collect();
        self
    }

    /// Switch to the v2 world_state responder (builder form). Tool responses are then
    /// derived from `ws` instead of static mocks.
    pub fn with_world_state(mut self, ws: Value) -> Self {
        self.responder = ResponderKind::WorldState(ws);
        self
    }

    /// Switch to the Phase-1 simulated-filesystem responder (builder form). `read_file` etc.
    /// then return real content from `fs` instead of static mocks / entity blobs.
    pub fn with_filesystem(mut self, fs: FsState) -> Self {
        self.responder = ResponderKind::FileSystem(fs);
        self
    }

    /// Switch to the Phase-2 web-search-corpus responder (builder form). `search`/`fetch` then
    /// return ranked snippets / real document text from `corpus`.
    pub fn with_web_corpus(mut self, corpus: CorpusState) -> Self {
        self.responder = ResponderKind::WebCorpus(corpus);
        self
    }

    /// Switch to the Phase-2 Slice-3 stateful web-UI responder (builder form). Holds only the
    /// immutable spec; the mutable per-run state lives in the runner.
    pub fn with_web_ui(mut self, spec: WebUiSpec) -> Self {
        self.responder = ResponderKind::WebUi(spec);
        self
    }

    /// The deterministic result for a parsed call. `StaticMocks`: `Some(mock)` or
    /// `None` for an unknown/hallucinated tool or wrong args (matched via `canonical`).
    /// `WorldState`: three-way — a GETTER surfaces the entity blob, a recognized ACTION
    /// acks `{"ok":true}`, and an unrecognized/decoy tool returns `None` so the runner
    /// injects the "unknown tool" nudge (a misleading ack would tell a model its decoy
    /// call succeeded, stalling it). An empty getter/recognized set means "all match"
    /// (legacy/back-compat), so the order below keeps v1 behavior intact.
    pub fn respond(&self, call: &Call) -> Option<String> {
        match &self.responder {
            ResponderKind::StaticMocks => self.mock_responses.get(&canonical(call)).cloned(),
            ResponderKind::WorldState(ws) => {
                if self.entity_tools.is_empty() || self.entity_tools.contains(&call.name) {
                    let r = crate::inference::eval::agentic::v2::world_state::derive_response(ws, call);
                    // A TAGGED getter that resolves nothing gets an honest miss, not a
                    // content-free ack: an ack reads as success and strands a model that
                    // queried a wrong/hallucinated entity id without its fact (the same
                    // acks-empty class the fs env fixed with `{"error":"not found"}`).
                    // The empty-set legacy arm keeps the v1 ack byte-identical.
                    if r == r#"{"ok":true}"# && !self.entity_tools.is_empty() {
                        return Some(r#"{"error":"not found"}"#.to_string());
                    }
                    Some(r)
                } else if self.recognized_tools.is_empty() || self.recognized_tools.contains(&call.name) {
                    Some(r#"{"ok":true}"#.to_string())
                } else {
                    None
                }
            }
            ResponderKind::FileSystem(fs) => fs.respond(call, &self.recognized_tools),
            ResponderKind::WebCorpus(c) => c.respond(call, &self.recognized_tools),
            // WebUi mutations are applied in the runner against the per-run state; this fallback
            // is only hit if a WebUi sandbox is driven through the stateless path (it isn't).
            ResponderKind::WebUi(_) => Some(r#"{"ok":true}"#.to_string()),
        }
    }
}

/// Per-RUN mutable fault state: how many times each trapped call has been
/// attempted so far. Lives outside `DeterministicSandbox` (which is immutable and
/// shared across the Pass^k runs) so each run starts with a clean slate — a
/// transient trap that cleared in run 1 traps again in run 2.
#[derive(Default)]
pub struct SandboxState {
    attempts: HashMap<String, u8>,
}

impl SandboxState {
    pub fn new() -> Self {
        Self::default()
    }

    /// The error text to inject for `call` on THIS attempt, or `None` when the call
    /// is untrapped or its transient fault has already cleared (so the real mock
    /// result should flow). Persistent faults always return text; transient faults
    /// return text for the first `clears_after` attempts, then clear. Counters are
    /// per-call (`canonical`), so multi-tool tasks trap independently.
    pub fn fault_for(
        &mut self,
        call: &Call,
        faults: &HashMap<String, FaultInjection>,
    ) -> Option<String> {
        // v1 faults key by `canonical(call)` (= "name|{args}", always has a '|');
        // v2 faults key by the bare tool name (no '|') and trip on ANY args. The two
        // key spaces never collide, so one map holds both: try exact-call first.
        let key = if faults.contains_key(&canonical(call)) {
            canonical(call)
        } else if faults.contains_key(&call.name) {
            call.name.clone()
        } else {
            return None;
        };
        match faults.get(&key)? {
            FaultInjection::PersistentError { status_code } => {
                Some(format!("HTTP {status_code} Fatal"))
            }
            FaultInjection::TransientError { status_code, clears_after } => {
                let n = self.attempts.entry(key).or_insert(0);
                if *n < *clears_after {
                    *n += 1;
                    Some(format!("HTTP {status_code} Service Unavailable"))
                } else {
                    None // cleared — let the deterministic mock result through
                }
            }
        }
    }
}

/// A stable string key for a call: tool name + its args with object keys sorted
/// recursively. `{"a":1,"b":2}` and `{"b":2,"a":1}` produce the same key, so a
/// model's arbitrary key ordering never causes a mock miss.
pub fn canonical(call: &Call) -> String {
    format!("{}|{}", call.name, canonical_json(&call.args))
}

fn canonical_json(v: &Value) -> String {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let inner: Vec<String> = keys
                .into_iter()
                .map(|k| {
                    format!("{}:{}", serde_json::to_string(k).unwrap_or_default(), canonical_json(&map[k]))
                })
                .collect();
            format!("{{{}}}", inner.join(","))
        }
        Value::Array(arr) => format!("[{}]", arr.iter().map(canonical_json).collect::<Vec<_>>().join(",")),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(name: &str, args: Value) -> Call {
        Call { name: name.into(), args }
    }

    fn sandbox() -> DeterministicSandbox {
        DeterministicSandbox::new(
            "Transfer the balance.".into(),
            vec![],
            vec![MockResponse {
                call: call("get_balance", json!({ "account_id": "ACC-123" })),
                response: r#"{"status":200,"balance":450.0}"#.into(),
            }],
            EndStateRule::RequireSequence(vec![TaskCheckpoint {
                tool: "execute_transfer".into(),
                args: json!({ "amount": 450.0 }),
            }]),
        )
    }

    #[test]
    fn respond_returns_mock_for_known_call() {
        let sb = sandbox();
        let got = sb.respond(&call("get_balance", json!({ "account_id": "ACC-123" })));
        assert_eq!(got.as_deref(), Some(r#"{"status":200,"balance":450.0}"#));
    }

    #[test]
    fn respond_is_none_for_unknown_tool_or_wrong_args() {
        let sb = sandbox();
        // Unknown / hallucinated tool.
        assert_eq!(sb.respond(&call("search_web", json!({ "q": "rates" }))).as_deref(), None);
        // Right tool, wrong args → still a miss (the sandbox is deterministic).
        assert_eq!(sb.respond(&call("get_balance", json!({ "account_id": "ACC-999" }))).as_deref(), None);
    }

    #[test]
    fn canonical_is_arg_order_insensitive() {
        let a = call("t", json!({ "a": 1, "b": 2 }));
        let b = call("t", json!({ "b": 2, "a": 1 }));
        assert_eq!(canonical(&a), canonical(&b));
    }

    #[test]
    fn action_tools_ack_while_getters_return_the_entity_blob() {
        // get_dep is a getter; pin_and_flag is an action. Both take the same entity id,
        // but only the getter surfaces the blob — the action acks (no answer-leniency).
        let sb = DeterministicSandbox::new(
            "p".into(),
            vec![],
            vec![],
            EndStateRule::RequireAll(vec![TaskCheckpoint { tool: "pin_and_flag".into(), args: json!({}) }]),
        )
        .with_world_state(json!({ "D-1": { "kind": "major" } }))
        .with_entity_tools(["get_dep".to_string()]); // only get_dep is a getter
        assert_eq!(
            sb.respond(&call("get_dep", json!({ "id": "D-1" }))).as_deref(),
            Some(r#"{"kind":"major"}"#)
        );
        assert_eq!(sb.respond(&call("pin_and_flag", json!({ "dep": "D-1" }))).as_deref(), Some(r#"{"ok":true}"#));
    }

    #[test]
    fn empty_getter_set_means_every_tool_returns_entity_data() {
        // Back-compat: no entity_tools → legacy behavior (every tool echoes the blob).
        let sb = DeterministicSandbox::new(
            "p".into(),
            vec![],
            vec![],
            EndStateRule::RequireAll(vec![TaskCheckpoint { tool: "t".into(), args: json!({}) }]),
        )
        .with_world_state(json!({ "D-1": { "kind": "major" } }));
        assert_eq!(sb.respond(&call("any_tool", json!({ "dep": "D-1" }))).as_deref(), Some(r#"{"kind":"major"}"#));
    }

    #[test]
    fn decoy_in_world_state_mode_returns_none_for_the_nudge() {
        // Three-way: get_dep (getter) → blob, pin_and_flag (recognized action) → ack,
        // read_file (decoy, not in the recognized whitelist) → None so the runner nudges
        // instead of falsely acking the decoy "success".
        let sb = DeterministicSandbox::new(
            "p".into(),
            vec![],
            vec![],
            EndStateRule::RequireAll(vec![TaskCheckpoint { tool: "pin_and_flag".into(), args: json!({}) }]),
        )
        .with_world_state(json!({ "D-1": { "kind": "major" } }))
        .with_entity_tools(["get_dep".to_string()])
        .with_recognized_tools(["get_dep".to_string(), "pin_and_flag".to_string()]);
        assert_eq!(sb.respond(&call("get_dep", json!({ "id": "D-1" }))).as_deref(), Some(r#"{"kind":"major"}"#));
        assert_eq!(sb.respond(&call("pin_and_flag", json!({ "dep": "D-1" }))).as_deref(), Some(r#"{"ok":true}"#));
        assert!(sb.respond(&call("read_file", json!({ "path": "x.py" }))).is_none());
    }

    #[test]
    fn respond_byte_parity_anchor_for_the_responderkind_refactor() {
        // GOLDEN: pins the EXACT bytes respond() returns for the pre-environment responder
        // kinds. Adding a ResponderKind variant (FileSystem/WebCorpus/WebUi) must NOT change
        // these — the model's next-turn transcript depends on them byte-for-byte. If this fails
        // after a responder refactor, the refactor drifted the baseline; fix the refactor, not
        // the assertion. (Fault/recovery transcript parity is pinned by the runner trap tests.)
        let mocks = sandbox();
        assert_eq!(
            mocks.respond(&call("get_balance", json!({"account_id":"ACC-123"}))).as_deref(),
            Some(r#"{"status":200,"balance":450.0}"#)
        );
        assert_eq!(mocks.respond(&call("nope", json!({}))), None);

        let ws = DeterministicSandbox::new(
            "p".into(),
            vec![],
            vec![],
            EndStateRule::RequireAll(vec![TaskCheckpoint { tool: "act".into(), args: json!({}) }]),
        )
        .with_world_state(json!({ "E-1": { "v": 1 }, "calc": { "2+2": 4 } }))
        .with_entity_tools(["get".to_string()])
        .with_recognized_tools(["get".to_string(), "act".to_string()]);
        assert_eq!(ws.respond(&call("get", json!({"id":"E-1"}))).as_deref(), Some(r#"{"v":1}"#));
        assert_eq!(ws.respond(&call("get", json!({"expr":"2+2"}))).as_deref(), Some("4")); // calc submap
        assert_eq!(ws.respond(&call("act", json!({"id":"E-1"}))).as_deref(), Some(r#"{"ok":true}"#));
        assert!(ws.respond(&call("decoy", json!({}))).is_none());
    }

    #[test]
    fn empty_recognized_set_keeps_legacy_ack_behavior() {
        // Back-compat guard: a task that sets getters but NOT a recognized whitelist must
        // keep acking unknown actions (old behavior), never nudging — so the new field
        // can't break existing passing tasks.
        let sb = DeterministicSandbox::new(
            "p".into(),
            vec![],
            vec![],
            EndStateRule::RequireAll(vec![TaskCheckpoint { tool: "pin_and_flag".into(), args: json!({}) }]),
        )
        .with_world_state(json!({ "D-1": { "kind": "major" } }))
        .with_entity_tools(["get_dep".to_string()]); // non-empty getters, empty recognized set
        assert_eq!(sb.respond(&call("anything", json!({ "dep": "D-1" }))).as_deref(), Some(r#"{"ok":true}"#));
    }

    #[test]
    fn world_state_mode_responds_with_the_entity_blob() {
        let sb = DeterministicSandbox::new(
            "p".into(),
            vec![],
            vec![],
            EndStateRule::RequireSequence(vec![TaskCheckpoint { tool: "t".into(), args: json!({}) }]),
        )
        .with_world_state(json!({ "M-3": { "ratio": 0.1 } }));
        assert_eq!(
            sb.respond(&call("compute_margin", json!({ "account": "M-3" }))).as_deref(),
            Some(r#"{"ratio":0.1}"#)
        );
    }

    #[test]
    fn respond_matches_despite_reordered_args() {
        let sb = DeterministicSandbox::new(
            "p".into(),
            vec![],
            vec![MockResponse { call: call("f", json!({ "x": 1, "y": 2 })), response: "ok".into() }],
            EndStateRule::RequireSequence(vec![TaskCheckpoint { tool: "done".into(), args: json!({}) }]),
        );
        assert_eq!(sb.respond(&call("f", json!({ "y": 2, "x": 1 }))).as_deref(), Some("ok"));
    }

    fn faults(rules: Vec<FaultRule>) -> HashMap<String, FaultInjection> {
        rules.into_iter().map(|r| (canonical(&r.call), r.fault)).collect()
    }

    #[test]
    fn transient_fault_clears_after_n_attempts() {
        let c = call("fetch", json!({ "id": 1 }));
        let f = faults(vec![FaultRule {
            call: c.clone(),
            fault: FaultInjection::TransientError { status_code: 503, clears_after: 2 },
        }]);
        let mut state = SandboxState::new();
        assert_eq!(state.fault_for(&c, &f), Some("HTTP 503 Service Unavailable".into())); // 1
        assert_eq!(state.fault_for(&c, &f), Some("HTTP 503 Service Unavailable".into())); // 2
        assert_eq!(state.fault_for(&c, &f), None); // cleared on the 3rd attempt
        assert_eq!(state.fault_for(&c, &f), None); // stays cleared
    }

    #[test]
    fn persistent_fault_never_clears() {
        let c = call("charge", json!({ "amt": 10 }));
        let f = faults(vec![FaultRule {
            call: c.clone(),
            fault: FaultInjection::PersistentError { status_code: 500 },
        }]);
        let mut state = SandboxState::new();
        for _ in 0..5 {
            assert_eq!(state.fault_for(&c, &f), Some("HTTP 500 Fatal".into()));
        }
    }

    #[test]
    fn fault_counters_are_per_call_independent() {
        let a = call("a", json!({}));
        let b = call("b", json!({}));
        let f = faults(vec![
            FaultRule { call: a.clone(), fault: FaultInjection::TransientError { status_code: 503, clears_after: 1 } },
            FaultRule { call: b.clone(), fault: FaultInjection::TransientError { status_code: 429, clears_after: 1 } },
        ]);
        let mut state = SandboxState::new();
        assert!(state.fault_for(&a, &f).is_some()); // a: attempt 1 → fails
        assert!(state.fault_for(&b, &f).is_some()); // b: attempt 1 → fails (independent of a)
        assert!(state.fault_for(&a, &f).is_none()); // a: cleared
        assert!(state.fault_for(&b, &f).is_none()); // b: cleared
    }

    #[test]
    fn name_keyed_fault_trips_on_any_args_and_clears() {
        // v2 `on_call: "mark_to_market"` → fails any call to that tool, transiently.
        let mut f: HashMap<String, FaultInjection> = HashMap::new();
        f.insert("mark_to_market".into(), FaultInjection::TransientError { status_code: 503, clears_after: 1 });
        let mut state = SandboxState::new();
        // Different args, same tool name — both share the one name-keyed counter.
        assert!(state.fault_for(&call("mark_to_market", json!({ "account": "M-3" })), &f).is_some()); // attempt 1
        assert!(state.fault_for(&call("mark_to_market", json!({ "account": "M-9" })), &f).is_none()); // cleared
        // A different tool is untouched.
        assert_eq!(state.fault_for(&call("other", json!({})), &f), None);
    }

    #[test]
    fn untrapped_call_never_faults() {
        let f = faults(vec![FaultRule {
            call: call("trapped", json!({})),
            fault: FaultInjection::PersistentError { status_code: 500 },
        }]);
        let mut state = SandboxState::new();
        assert_eq!(state.fault_for(&call("safe", json!({})), &f), None);
    }

    #[test]
    fn with_faults_keys_by_canonical_form() {
        let sb = sandbox().with_faults(vec![FaultRule {
            call: call("get_balance", json!({ "account_id": "ACC-123" })),
            fault: FaultInjection::PersistentError { status_code: 500 },
        }]);
        // Reordered args still resolve to the same fault key.
        assert!(sb.faults.contains_key(&canonical(&call("get_balance", json!({ "account_id": "ACC-123" })))));
    }
}
