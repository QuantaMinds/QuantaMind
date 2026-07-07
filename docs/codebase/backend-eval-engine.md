# Backend — Evaluation Engine

The local-LLM evaluation subsystem: deterministic tool-call scoring, a sandboxed
multi-step agentic runner, a stoppable Context Stress Test, readiness / VRAM-fit
synthesis, and a crash-resumable batch queue. All inference-side logic is
Tauri-free and pure where possible; the `commands/eval/` layer is the thin IPC
skin that streams events and persists results.

Cross-links:
- Persistence of eval history, traces, cliff results, and readiness profiles →
  `backend-persistence.md`.
- The Eval UI (datasets, run, scoreboard, trajectory inspector) → `frontend-eval.md`.
- Readiness / Agent-Report / Latency UI → `frontend-inspector-quant-agentreport.md`.

---

## Overview

### Why eval exists

Small local quants are unreliable at *structured* tasks — emitting a clean tool
call, selecting the right tool, getting args right, abstaining when no tool is
needed, staying coherent over multiple steps, and holding accuracy as context
grows. The eval engine measures these deterministically (no LLM judge, no
execution side effects) so the numbers are reproducible and never fabricated:
every metric is either a measured value or `None` (rendered "N/A"/"—"), never an
estimate or a 0 substituted for missing data.

### The four eval modes + readiness

| Mode | Entry | What it measures | Scoring |
|------|-------|------------------|---------|
| **Single tool-call** | `run_toolcall_eval` | One model over a collection: parse / tool-select / arg / abstain | `toolcall::score` + cascaded `aggregate` |
| **Model Results** | `run_collection_matrix` | Same collection across N models (sequential) | per-column `ToolCallReport` + mean composite |
| **Agentic** | inside `run_batch_eval` (tasks `category == "agentic"`) | Multi-step sandboxed tool loop, Pass^k reliability | `agentic::report::AgenticReport` |
| **Context Stress Test** | `run_context_cliff` | Largest verified prompt-token depth before accuracy collapses | per-rung composite vs baseline |
| **Readiness** | `assess_readiness` | A measured batch report + cliff + VRAM fit vs a use-case profile → Ready/Conditional/NotReady | `readiness::verdict::assess` |

The **batch** mode (`run_batch_eval`) is the umbrella runner: it mixes single-turn
and agentic tasks across multiple models, isolates VRAM between models, streams
per-task/per-step progress, and is crash-resumable.

### How — IPC commands (all `#[tauri::command]`)

| Command | Module | Purpose |
|---------|--------|---------|
| `run_toolcall_eval` | toolcall_cmd | single-model tool-call eval over a collection |
| `trace_toolcall_task` / `load_toolcall_trace` | toolcall_cmd | live / cached single-task trace |
| `get_builtin_tasks` / `list_builtin_collections` / `get_builtin_collection` | toolcall_cmd | bundled preset catalog |
| `run_eval_task` | eval_run | one generic `EvalTask` (exact / MC / JSON-schema) |
| `list_evals` | evals_load | bundled `*.yaml` generic evals |
| `run_collection_matrix` / `load_collection_history` | matrix_cmd | N-model matrix + history |
| `run_batch_eval` / `stop_batch_eval` | batch_cmd | streaming batch run + cancel |
| `check_unfinished_run` / `resume_batch_eval` / `discard_run` | batch_cmd | crash-resume queue |
| `run_context_cliff` / `stop_context_cliff` | readiness_cmd | stoppable cliff probe + cancel |
| `save_cliff_result` / `get_cliff_results` | readiness_cmd | persisted cliff verdicts |
| `assess_readiness` | readiness_cmd | profile-gated verdicts |
| `list_readiness_profiles` / `save_readiness_profile` / `delete_readiness_profile` | readiness_cmd | profile CRUD |
| `list_custom_collections` / `load_custom_collection` / `save_custom_collection` / `delete_custom_collection` / `import_custom_collection` / `read_text_capped` | eval_registry | custom collection CRUD + import |

---

## Folder: `inference/eval/` (root)

### File: `mod.rs`
- **Responsibility:** Module root. **Why:** single import surface. **What:**
  `pub mod agentic; batch; cliff; eval_score; eval_task; readiness; toolcall;`.

### File: `eval_task.rs`
- **Responsibility:** The generic bundled-eval task type + its deterministic
  scoring rule enum.
- **Why:** A second, simpler eval family (`docs/evals/*.yaml`) independent of the
  tool-call engine — exact-match / multiple-choice / flat JSON-schema.
- **What:** `EvalTask { id, category, prompt, scoring: Scoring }`; tagged enum
  `Scoring::{ Exact{expected}, MultipleChoice{choices,expected}, JsonSchema{required,types} }`.
- **How/Where used:** loaded by `evals_load::load_all`, scored by `eval_score::score`,
  run by `eval_run::run_and_score`.

```rust
#[serde(tag = "method", rename_all = "snake_case")]
pub enum Scoring {
    Exact { expected: String },
    MultipleChoice { choices: Vec<String>, expected: String },
    JsonSchema { required: Vec<String>, #[serde(default)] types: BTreeMap<String, String> },
}
```

### File: `eval_score.rs`
- **Responsibility:** Pure deterministic scorer for generic `EvalTask`s.
- **Why:** No judge, no execution — a green test means the *output* matched the
  rule in shape and value.
- **What:** `EvalScore { passed, detail }`; `pub fn score(task, output) -> EvalScore`;
  `pub fn first_json_value(text) -> Option<Value>` (greedy balanced-brace extractor);
  privates `balanced_from`, `strip_fences`, `score_json` (flat depth-1 check),
  `first_choice` (whole-word token match), `type_matches`.
- **How/Where used:** `eval_run::run_and_score`.

**Greedy multi-object JSON extraction with balanced-brace detection** — scans for
the *first* `{` whose balanced slice actually parses, skipping prose braces:

```rust
pub fn first_json_value(text: &str) -> Option<Value> {
    for (i, &b) in text.as_bytes().iter().enumerate() {
        if b == b'{' {
            if let Some(slice) = balanced_from(text, i) {        // string/escape-aware
                if let Ok(v) = serde_json::from_str::<Value>(slice) { return Some(v); }
            }
        }
    }
    None
}
```

**Flat (depth-1) JSON-conformance** — required keys present + declared top-level
types match; no recursion into nested objects/arrays:

```rust
for key in required { if !obj.contains_key(key) { return fail(format!("missing key: {key}")); } }
for (key, ty) in types {
    match obj.get(key) {
        Some(v) if type_matches(v, ty) => {}
        Some(_) => return fail(format!("key '{key}' is not {ty}")),
        None    => return fail(format!("missing typed key: {key}")),
    }
}
```

---

## Folder: `inference/eval/toolcall/`

### File: `mod.rs`
- **Responsibility:** Module root: `eval, matrix, parse, prompt, score, tasks`.

### File: `tasks.rs`
- **Responsibility:** The tool-call task model + the trust-boundary validator + the
  bundled presets.
- **Why:** One validation gate for *any* collection source (built-in, saved,
  imported, hand-edited); validate untyped `Value` schemas by deserializing into a
  strict struct, not hand traversal.
- **What:** `ToolSchema{name,description,parameters}`, `Call{name,args}`,
  `Expected::{Call, Parallel{calls}, NoCall}` (with `.calls() -> Option<&[Call]>`),
  `ToolTask{id,category,prompt,tools,expected,agentic: Option<AgenticSpec>}`;
  `pub fn validate_tasks(&[ToolTask]) -> AppResult<()>`; preset loaders
  `tasks()`/`finance_tasks()`/`agentic_tasks()`/`agentic_{3,5,8}_tasks()` (each
  `include_str!` a `tasks*.json`); `BUILTIN_COLLECTIONS: &[(&str,&str)]`;
  `builtin_collection(id) -> Option<Vec<ToolTask>>`. Categories are
  `["single","parallel","select","abstain","agentic"]`.
- **How/Where used:** validated by every run command; presets served by
  `toolcall_cmd`.

```rust
let p: StrictParameters = serde_json::from_value(tool.parameters.clone())  // serde does the work
    .map_err(|e| bad(id, &format!("tool '{}' parameters", tool.name), &e.to_string()))?;
if p.schema_type != "object" { return Err(bad(id, …, "type must be \"object\"")); }
// abstain ⇔ no_call: category and expected must agree
if abstain != matches!(t.expected, Expected::NoCall) { return Err(bad(&t.id, "expected", "abstain ⇔ no_call")); }
```

### File: `prompt.rs`
- **Responsibility:** Build the tool-call system prompt (tool schemas as JSON +
  "respond with ONLY a JSON object/array").
- **Why:** Explicit instruction so a weak model's format failures surface as a low
  `parse_rate` (signal, not noise).
- **What:** `build_system(task)` → `build_system_for(tools, terminal)` (shared with the
  agentic runner, which has a sandbox not a `ToolTask`). `terminal: TerminalGuidance`
  gates the CLOSING line (G1): `MustUseTools` for act-tasks (`RequireAll`/`RequireSequence`),
  `PlainTextOk` for abstain (`ExpectAbstainingText`) + single-turn + cliff (keeps the
  plain-text option). Without this, a reporter task that requires a terminal `reply` call
  contradicted the prompt's plain-text invitation and failed a correct prose answer.
  `MustUseTools` is **reply-tool-AWARE** via `reply_tool_name(tools)` (first tool with a
  `text` param): a reporter task names the REAL tool ("call the `reply_customer` tool" —
  fixing the `reply`/`reply_customer` misnomer); an action-only task is told "your tool
  actions are your final answer" so the mandate never points at a nonexistent `reply` tool
  (which had induced a phantom `reply` call → schema-error/timeout on ~55 action-only tasks).
  Guarded by `scenarios.rs::reply_tool_name_classifies_every_task_and_reporters_are_unique`
  (≤1 text-bearing tool per task; reporter checkpoint ⇒ that tool; action-only ⇒ `None`).

```rust
"You can call tools. Available tools:\n{tools_json}\n\n\
 When a tool is needed, respond with ONLY a JSON object of the form \
 {{\"name\": \"<tool>\", \"args\": {{...}}}}. To call several tools, respond with a JSON array …"
```

### File: `parse.rs`
- **Responsibility:** Extract tool calls from a raw completion, across model dialects.
- **Why:** Small quants emit calls as a single object, a JSON array, bare
  `{..}\n{..}` sequences, or echo the call inside a fence — all must parse, and
  duplicates must collapse so the cardinality guard doesn't false-fail. Some models
  (gemma) ignore the JSON instruction entirely and emit their NATIVE channel grammar;
  without normalization those score as `MalformedJson` forever, for the full slow
  Pass^k grind.
- **What:** `extract_calls(completion) -> Option<Vec<Call>>` (delegates to
  `extract_calls_dialect`); `extract_calls_dialect(completion) -> Option<(Vec<Call>, ToolCallDialect)>`
  tries the instructed JSON first (`extract_standard`), then falls back to
  `harmony_calls` (the `call:NAME{ bare: "args" }` channel format). Privates `objects()`
  (every top-level balanced `{…}`, string/escape aware), `to_call()`
  (`{name, args|arguments}`), `balanced_brace`, `relax_object`/`relax_to_json` (quote bare
  keys + escape raw control chars in code strings so a JS-object body parses);
  `pub(crate) has_json_object`, `looks_like_broken_json` (used by the agentic runner to
  tell broken JSON from a hallucinated completion), `pub(crate)
  looks_like_foreign_dialect` (a channel control token co-occurring with an attempted
  `call:IDENT{`/`(` structure — see below), and `pub(crate) is_empty_output` (no
  alphanumeric char → the model produced nothing usable → `EmptyOutput`, not
  `Hallucinated`).
- **Dialect (`ToolCallDialect`):** `Standard` (instructed JSON) or `Harmony` (native
  channel grammar). The runner threads the recovered dialect onto `RunOutcome`→`AgenticReport`
  so the scoreboard flags a model that only scored via normalization (transparency — the
  score isn't silently laundered). Standard is tried first, so a clean-JSON model is never
  mislabeled; a bareword VALUE still fails to relax → dropped, not guessed.
- **Foreign-dialect detection — `looks_like_foreign_dialect` (production parity):** a
  mis-built model can emit an unparseable non-JSON dialect (harmony-ish soup like
  `<|tool_response|>call:reply(text='…')`, or `call:reply{text:<|"|>…<|"|>}`). The harness
  does NOT salvage these — `harmony_calls` already drops them (paren form has no `{`; the
  `<|"|>`-wrapped body fails `relax_object`), matching what Ollama's native parser also
  drops, so the bench never scores looser than a real deployment. Instead the runner labels
  such a no-call turn `ForeignDialect` (its own `FailureKind`/`StepKind`/`TopError`/tally),
  distinct from `Malformed`/`Hallucinated`. The detector requires BOTH a channel control
  token (`<|tool_response`/`<channel|`/`<tool_call|`) AND an attempted-call structure, so
  prose merely quoting those tokens is never mislabeled. (`foreign_dialect_calls` is
  internal-only; not on the publish allowlist — see `backend-publish.md`.)
- **Empty-output detection — `is_empty_output`:** a no-call turn whose text has no
  alphanumeric char (empty / whitespace / a lone `.` before the stop token — gemma-qat's
  prompt-path symptom) is labeled `EmptyOutput`, checked FIRST in the no-call arm, distinct
  from `Hallucinated`. Such a model usually needs the native tools path (it returns clean
  `tool_calls` natively). Also internal-only (not on the publish allowlist).
- **How/Where used:** `eval::trace_one_with`, `runner::run_steps`.

```rust
for call in objects(&cleaned).into_iter()
    .filter_map(|slice| serde_json::from_str::<Value>(slice).ok())
    .filter_map(to_call) {
    if !calls.contains(&call) { calls.push(call); }   // dedup identical calls; keep distinct parallel
}
(!calls.is_empty()).then_some(calls)                   // None ⇒ abstention is scoreable
```

### File: `score.rs`
- **Responsibility:** Pure per-task verdict from `(expected, parsed)`.
- **Why:** Deterministic, length-guarded, order-independent matching that the
  agentic end-state matcher reuses (same `args_match`).
- **What:** `Verdict{parsed,tool_match,args_match,abstain_correct: Option<bool>}`;
  `pub fn score(expected, parsed) -> Verdict`; `pub(crate) verdict_passed(&Verdict)`;
  `pub(crate) args_match(expected, got)` (same key set, values equal — numbers
  numerically, strings trimmed); privates `bijection` (1:1 greedy assignment),
  `set_match` (length-guard then tool then args).
- **How/Where used:** `eval::trace_one_with`, `batch.rs`, `endstate.rs`.

```rust
fn set_match(expected: &[Call], parsed: &[Call]) -> (bool, bool) {
    if expected.len() != parsed.len() { return (false, false); }   // length guard: extra/missing → fail
    let tool = bijection(expected, parsed, |e, p| e.name == p.name);
    let args = bijection(expected, parsed, |e, p| e.name == p.name && args_match(&e.args, &p.args));
    (tool, args)
}
```

### File: `eval.rs`
- **Responsibility:** Run a collection task-by-task, keep full traces, aggregate
  into a `ToolCallReport` with **cascaded conditional denominators**.
- **Why:** A format failure must never bleed into the reasoning metrics — each
  metric's denominator is conditioned on the prior stage succeeding.
- **What:** `TaskResult`, `TraceResult` (system msg + prompt + raw output + verdict
  + real `prompt_eval_count`), `TaskTrace`, `ToolCallReport`; `pub(crate) aggregate`;
  `trace_one_with`/`trace_one` (live backend, greedy temp 0, `MAX_TOKENS=256`);
  `run_eval_traced` (report + persisted traces) and thin `run_eval`.
- **How/Where used:** `toolcall_cmd`, `matrix_cmd`, `batch.rs`, `cliff::engine`.

**Cascaded conditional denominators** (each metric over the subset that reached
that stage; each `None` when its denom is 0, never 0):

```rust
let parse_den = tasks.iter().filter(|t| call(t)).count();             // tasks expecting a call
let parse_num = z().filter(|(t,r)| call(t) && r.verdict.parsed).count();
let tool_num  = z().filter(|(t,r)| call(t) && r.verdict.parsed && r.verdict.tool_match).count();
let arg_den   = results.iter().filter(|r| r.verdict.tool_match).count();   // tool-matched tasks
let arg_num   = results.iter().filter(|r| r.verdict.tool_match && r.verdict.args_match).count();
let ab_den    = tasks.iter().filter(|t| !call(t)).count();            // NoCall tasks
// …
let tool_selection_acc = rate(tool_num, parse_num);                   // denom = PARSED call-tasks
let composite = (!subs.is_empty()).then(|| subs.iter().sum::<f64>() / subs.len() as f64);
```

#### Scoring metrics & denominators

| Metric | Numerator | Denominator | `None` when |
|--------|-----------|-------------|-------------|
| `parse_rate` | call-tasks that parsed | tasks expecting a call | no call-tasks |
| `tool_selection_acc` | parsed call-tasks with right tool | **parsed** call-tasks | nothing parsed |
| `arg_acc` | tool-matched with right args | **tool-matched** tasks | nothing tool-matched |
| `abstain_acc` | correctly abstained | NoCall (abstain) tasks | no abstain tasks |
| `composite` | — | mean of the available sub-scores above | all sub-scores absent |
| `prompt_tokens` | — | mean of real per-task `prompt_eval_count` | none reported |

### File: `matrix.rs`
- **Responsibility:** Fold per-target outcomes into a `MatrixReport` (pure).
- **Why:** A down backend must become that column's error, not abort the matrix.
- **What:** `ModelTarget{model,backend}`, `MatrixColumn{model,backend,report,error}`,
  `MatrixReport{collection_id,columns,avg_score}`; `build_matrix(...)`,
  `summaries(report, ts) -> Vec<RunSummary>` (only successful columns recorded).
- **How/Where used:** `matrix_cmd::run_collection_matrix`.

---

## Folder: `inference/eval/agentic/`

The agentic runner is a **sandboxed multi-step tool loop**: the model emits
raw-text JSON tool calls, a deterministic sandbox replies in text (no native
function-calling required, so it runs identically across Ollama/llama.cpp/MLX),
and the loop runs `k` times for Pass^k reliability.

**Thinking models.** The per-turn output budget is normally `num_predict = 256`
(`difficulty::passk::NON_THINKING_MAX_TOKENS`), enough for a tool call. A reasoning
model marked `is_thinking` (per-model, from `ModelTarget` → `BackendTurn`) instead
gets a tier-scaled budget (`max_tokens_for`: 1536/2048/3072/4096) so its
`<think>…</think>` scratchpad doesn't truncate the call, and the runner strips
`<think>` (via `toolcall::parse::strip_think`) **before parsing and before the
transcript append** — the scratchpad's inner JSON can't be mis-parsed as a call and
never bloats the prefix-KV context. The streamed `raw_output` keeps the full text so
the UI still shows the reasoning. The flag is carried onto `BatchColumn`/`RunSummary`
because a thinking model's `effort` (output tokens) is higher by design and must not
be ranked against a terse model's.

**Reasoning-budget context on the verdict + publish.** `BatchColumn` also carries the
per-model `cpu_offloaded` and hardware-adaptive `ctx_ceiling` (stamped by `batch_cmd`
after the run from the placement probe + `agentic_ctx_ceiling` band), and `BatchReport`
carries the batch-wide `think_preset` (Lean/Standard/Deep). `verdicts_for_column` threads
all four (`is_thinking`, `cpu_offloaded`, `ctx_ceiling`, `think_preset`) onto every
`ModelVerdict`, so the Agent Report shows the thinking budget and the publish payload can
carry it (`PublishRow` schema v2; `think_budget` = `think_tokens_for_preset(tier_tested,
preset)` for a reasoning model). The runner also populates each step's `reasoning_tokens`
from the measured generated-token count on a thinking turn (not just on a `Truncated`/
`ReasoningOverrun` turn), so the Trace Debugger can sum "how much it thought" per run.

### File: `mod.rs`
- Declares `build, context, endstate, model_turn, report, runner, sandbox, spec, step`.

### File: `spec.rs`
- **Responsibility:** Serde task definition for agentic runs (mocks + success rule
  + Pass^k/step/recovery/fault overrides), carried as `ToolTask.agentic`.
- **What:** `FaultInjection::{TransientError{status_code,clears_after}, PersistentError{status_code}}`,
  `FaultRule{call,fault}`,
  `AgenticSpec{mocks, end_state: EndStateRule, k, max_steps, faults, max_recovery}`.

### File: `sandbox.rs`
- **Responsibility:** The deterministic simulated tool environment + per-run fault
  state.
- **Why:** Mocks keyed by **canonical** (sorted-key) form so arg ordering never
  causes a miss; one environment shared (immutable) across all `k` runs.
- **What:** `MockResponse{call,response}`, `TaskCheckpoint{tool,args}`,
  `EndStateRule::{RequireSequence(Vec<TaskCheckpoint>), ExpectAbstainingText}`,
  `DeterministicSandbox` (`new`, `with_faults`, `with_entity_tools`, `with_recognized_tools`, `respond(&Call) -> Option<&str>`),
  `SandboxState{attempts}` (`fault_for`), `pub fn canonical(&Call) -> String`.
- **Getter vs action vs decoy (v2 WorldState):** `respond` is three-way. A GETTER (in
  `entity_tools`, the authored `returns_entity` set) surfaces the world_state entity blob
  (`derive_response`: calc sub-map first, then first entity-keyed string arg, then the
  blob authored under the tool's own name — the fallback that grounds computation/no-arg
  getters like `run_import_check{}`); a recognized ACTION (in `recognized_tools`, the authored real-tool
  whitelist, but not a getter) gets a generic `{"ok":true}` ack — so an action can't echo
  the field the model was supposed to reason to (answer-leniency); an UNRECOGNIZED tool (a
  decoy or hallucination, in neither set) returns `None` → the runner injects the
  `UNKNOWN_TOOL` nudge. The nudge (not a misleading `{"ok":true}` "success") lets a capable
  model recover from taking a decoy in one turn instead of stalling on a contentless ack
  until the per-step timeout. Both sets EMPTY means "every tool matches" (v1 / legacy
  back-compat); the order keeps v1 behavior byte-identical. The whitelist is built in
  `build::sandbox_for` — from `task.tools` for v1 (decoy-free there) or
  `spec.recognized_tools` for v2 (whose `task.tools` already carries the merged decoys).
- **Stateful web-UI env (Slice 3):** `ResponderKind::WebUi(WebUiSpec)` holds only the IMMUTABLE
  initial state; the MUTABLE `WebUiState` (`v2/env_webui.rs`) is per-run — constructed fresh in
  `run_steps` beside the fault `SandboxState`, NEVER in the shared sandbox. `apply(call)` mutates on
  `fill`/`toggle`/`navigate`/`click`/`submit` (a pure `(state,action)->state'`); the runner branches
  to it instead of `respond`. `EndStateRule::RequireEndState(target)` grades on the final state
  exactly matching the target (no partial credit), evaluated AFTER the forbidden pre-scan so
  `ForbiddenCall` still dominates. Authored via `target_state` in v2 JSON (→ transpile);
  `expected_calls` then drive only the oracle. `EnvView::WebUi` is rebuilt POST-action.
- **`ResponderKind` (enum, not trait) + deterministic envs:** `respond` dispatches an exhaustive
  `match` over `StaticMocks` / `WorldState(Value)` / `FileSystem(FsState)` / `WebCorpus(CorpusState)`
  / `WebUi(WebUiSpec)` (the WebUi arm is a no-op fallback — real mutations run in the runner).
  Adding a variant is compile-forced everywhere; `StaticMocks`/`WorldState` output is pinned
  byte-identical by `respond_byte_parity_anchor_for_the_responderkind_refactor`. The
  `FileSystem` arm (`v2/env_fs.rs`, Phase 1) returns REAL content for `read_file`/`list_dir`/
  `search_files`/`grep`; the `WebCorpus` arm (`v2/env_corpus.rs`, Phase 2) returns COMPUTED
  deterministically-ranked snippets for `search` and real document text for `fetch` (or a
  deterministic `{"error":"not found"}`) — both the acks-empty fix. Built from `world_state`
  (fs: `path→content`; corpus: `doc_id→{title,text}`) per the collection's `"environment"`
  (`spec::EnvKind` ∈ `Entity` default / `Filesystem` / `WebCorpus`). Stateless-per-call →
  determinism is structural; corpus ranking is distinct-term-match, tied by `doc_id`, snippet =
  first query-term line top-to-bottom else head (pure, on the replay-vs-score surface).
- **`env_view.rs` (visual replay spine):** `EnvView{None, FileSystem(FsView), WebCorpus(CorpusView)}`
  is a per-turn snapshot streamed on each `TrajectoryStep` (`#[serde(default)]`). `env_view(
  responder, calls)` (exhaustive match) is a PURE fn of the immutable env + the turn's calls (each
  env picks the last getter call, so a `read_file`/`search` batched before a `reply` still
  replays). The corpus view is **lazy** — only the index (id+title) per turn; full text rides along
  only for the fetched doc. Internal/local-only — NOT on the publish allowlist.

```rust
FaultInjection::TransientError { status_code, clears_after } => {
    let n = self.attempts.entry(key).or_insert(0);
    if *n < *clears_after { *n += 1; Some(format!("HTTP {status_code} Service Unavailable")) }
    else { None }   // cleared — the deterministic mock result flows through
}
```

### File: `build.rs`
- **Responsibility:** Project an agentic `ToolTask` into `(DeterministicSandbox, AgenticConfig)`.
- **Why:** Defense-in-depth — re-checks every checkpoint/mock/fault names a declared
  tool before running.
- **What:** `sandbox_for(task) -> AppResult<(DeterministicSandbox, AgenticConfig)>`;
  fills config from spec overrides. An absent `k`/`max_steps` falls back to the **tier
  policy** (`passk::pass_k_for` / `passk::max_steps_for`), NOT a flat default: a harder
  tier gets both more Pass^k runs and a longer step horizon —
  `k` = 5/8/16/24 and `max_steps` = 8/16/32/48 for Easy/Medium/Hard/Extreme. An explicit
  authored value (or the UI K / Max-Steps field) always wins. `max_recovery` still
  defaults from `AgenticConfig::default()` (2).

### File: `context.rs`
- **Responsibility:** The running transcript (initial prompt + alternating model
  turns and injected tool results).
- **Why:** Single source so the streamed `injection` text matches what the model
  sees; `reset` isolates each Pass^k iteration.
- **What:** `tool_result_line(data) -> "Tool result: {data}"`;
  `Conversation{initial_prompt,turns}` (`push_model`, `push_tool_result`, `render`, `reset`).

### File: `model_turn.rs`
- **Responsibility:** The `ModelTurn` seam (prompt → text + stats) and its impls.
- **Why:** The runner depends on the trait, not a backend, so it's unit-testable
  with a scripted model; the native path translates structured `tool_calls`
  back into canonical `{name,args}` JSON so sandbox/scoring stay byte-identical
  — across both passes AND across backends.
- **What:** `trait ModelTurn { async fn run(&self, &GenerateSpec) -> …; async fn warm_up(&self) -> AppResult<()> { Ok(()) } }`;
  `BackendTurn{backend,endpoint,model,cancel,options,keep_alive}` (dispatch by
  `BackendKind`); `NativeToolTurn{backend,endpoint,model,tools,options,…}` — native
  FC follows the running server, dispatching `chat_with_tools` by `BackendKind`
  (Ollama `/api/chat`, llama.cpp OpenAI `/v1/chat/completions`; MLX has none),
  then `synthesize_calls` for the shared canonical text.
- **Options merge (`merge_eval_options`):** `BackendTurn::run` merges the header's global
  eval params with the harness per-turn spec **field-wise** — it does NOT let one replace the
  other. The loop's structural caps win: `num_predict` (the `MAX_TOKENS=256` anti-runaway
  cap) and `num_ctx` (sized by `agentic_num_ctx`) take the spec's value, with the header as
  fallback; sampling prefs (temperature/top_p/top_k/repeat_penalty/seed) take the header's
  value, with the spec as fallback. **Why it matters:** the prior `global.or(spec)` replaced
  the whole options object, so a header that omitted `max_tokens` (`num_predict: None`)
  stripped the per-turn cap → unbounded generation (minutes/turn, a giant multi-call blob)
  AND dropped the sized `num_ctx` → context-shift that busts the prefix-KV cache. Both the
  256 cap and the cache fix (`b97d79c`) were silently dead whenever the header sent params.
- **Per-model stop tokens (`resolve_model_stops`):** after the merge, `BackendTurn::run`
  fills `options.stop` (when unset) with the model's real end-of-turn markers, resolved once
  per run (memoized in `BackendTurn.stop_cache`) from Ollama `/api/show`
  `general.architecture` → `detect_template().stop_tokens`. **Why it matters:** harmony
  models (gpt-oss) end turns on `<|return|>`/`<|call|>` and gemma on `<end_of_turn>` — none a
  plain EOS — so without stops they emit the markers as literal text and run to the token cap,
  hallucinating a whole multi-turn transcript (the infinite-generation bug). `/api/show` is
  metadata-only (no weight load → no model-switch latency); unknown families resolve to `[]`
  (prior no-stop behavior). Ollama-only for now (llama.cpp/MLX is a follow-up). NOTE: this
  cures gpt-oss's loop; gemma4's separate pad-token collapse is unrelated and unfixed here.
- **Anti-collapse repeat penalty (`EVAL_REPEAT_PENALTY = 1.1`):** every eval spec
  (agentic `runner`, single-turn `toolcall`, `cliff` probe) sets
  `repeat_penalty: Some(EVAL_REPEAT_PENALTY)` from the shared constant in
  `generate/generate_options.rs`. Greedy eval (`temperature 0.0`) with no penalty let
  loop-prone quants (gpt-oss/gemma) run to the token cap repeating one phrase — surfacing as a
  `HallucinatedCompletion` (a misnomer: no tool call parsed, not a fake stop token). The UI's
  "Repeat penalty" 1.1 was only a *placeholder*; `globalParams` starts `{}` and omits untouched
  keys, so the backend got no penalty unless the slider was dragged. The harness default fixes
  that for eval only (the chat playground keeps "unset = backend default"); a header-supplied
  value still wins via the merge above. One constant, three call sites, so the value can't drift
  and the metric stays comparable across models.
- **Warm-up:** `run_batch_resumable` calls `turn.warm_up()` once per model before its
  first scored task. `BackendTurn` issues a 1-token generate (honoring `keep_alive`) to
  load the weights resident, so cold-load latency isn't charged to the first task as a
  `TurnTimeout` (which would penalize every model's first task). Default is a no-op
  (scripted models / backends that can't warm just run cold, as before). Note: warm-up
  loads *weights* — a runtime that allocates more on first *large* context (cliff probe)
  could still pay that on the first rung; out of scope here.
- **Keep-alive floor:** `batch_cmd::agentic_keep_alive` floors the batch `keep_alive`
  at `AGENTIC_KEEP_ALIVE_SECS=600` when the UI leaves it unset (an explicit value, incl.
  `-1`=forever, still wins). Without it Ollama's default 5-min idle unload can evict the
  model — and its prefix-KV cache — across an inter-task/inter-turn gap mid-batch, so the
  warm-up's load wouldn't survive the ~k×max_steps sequential calls.
- **Context window (`num_ctx`):** the agentic loop re-sends the whole growing transcript
  every step; `runner::agentic_num_ctx(max_steps)` sizes `num_ctx` to
  `clamp(2048 + max_steps·384, 4096, 16384)`. Left at the model default (~4096) a
  multi-step transcript overflows → Ollama context-shift, which busts the automatic
  prefix-KV cache (full re-prefill every turn — the stall) AND silently drops the
  earliest turns.

### Reasoning-model budget handling (D1–D9)
A reasoning model's `<think>` scratchpad is charged against the SAME `num_predict` as the
answer, and Ollama routes it into a separate `thinking` field. If the harness ignores that and
the budget is too small, reasoning eats the whole cap and the answer comes back empty — a
capable model is then mis-scored `Truncated`. Proven live: qwen3.5:9b spent ~3700/4096 tokens
thinking → empty answer. The fix, across the runner / `model_turn` / `difficulty::passk` /
`hwclass` / `report` / `step`:
- **Capture the reasoning (D1):** both backends stream the scratchpad in a SEPARATE field, which
  `stream_generate` reads and re-emits wrapped in inline `<think>…</think>` so the runner's
  `strip_think` + the D9 accounting handle every backend uniformly. Ollama: `think:true` → the
  `thinking` field. **llama.cpp: modern llama-server (`--jinja`, default `--reasoning-format`)
  EXTRACTS `<think>` out of `content` into a `reasoning_content` field** — it does NOT leave it
  inline, contrary to an earlier assumption. Proven live: qwen3.5-9b emitted 187 `reasoning_content`
  chunks with 0 captured until `ChatDelta.reasoning_content` + the `<think>`-wrap were added
  (`llama::stream_chat`); after the fix, 14 `<think>` turns captured on the same collection. A terse
  model, or `--reasoning-format none` (which keeps `<think>` inline), sends no `reasoning_content`
  and the wrap is a no-op. **MLX is unverified** — its template may emit untagged reasoning, in which
  case `strip_think` no-ops and MLX would regress; a known gap until an MLX reasoning model can be
  inspected (`inference/mlx`). The lesson: reasoning-channel behavior MUST be verified per backend
  live, never assumed from another backend's docs.
- **Fixed per-tier budget, NOT hardware-scaled (D2):** `passk::think_tokens_for_preset(tier,
  ThinkPreset)` — `Lean/Standard/Deep` presets, each a constant. Reasoning length is a property of
  the task and model, not the tester's RAM; scaling the budget by hardware would make the same
  model pass a tier on a big box and fail on a small one → a non-reproducible tier. Values are
  placeholders until a cross-model histogram (P95 of the chattiest model per tier) locks them.
- **Only `num_ctx` is hardware-adaptive (D3):** `agentic_num_ctx(max_steps, is_thinking, ceiling)`
  gives a thinking model the WHOLE window; `ceiling = hwclass::agentic_ctx_ceiling(total_ram)`
  (Constrained 8192 · Mainstream 16384 · Workstation 32768 · Frontier 65536), threaded onto
  `BackendTurn.ctx_ceiling` from the batch command's hardware snapshot. Hardware decides *whether*
  the fixed budget fits, never *how much* the model may think; a box too small yields an honest
  `Truncated (context-bound)`. (Precise per-model ceiling + llama.cpp launch-`-c` lockstep are a
  tracked follow-up; the class band covers the Ollama case.)
- **Honest labels (D7/D9):** on a length-cut, zero-call turn the runner classifies with usage
  numbers (`stats.eval_count` vs `base_predict`; `prompt_eval_count + cache_n` vs `num_ctx`):
  `FailureKind::ReasoningOverrun` when the per-turn BUDGET capped generation while the window had
  room (a SETTING — raise the preset), vs `FailureKind::Truncated` when the window filled (context
  -bound, HARDWARE) or usage is unknown (the honest default — never assert over-reasoning blind).
  The two must never be blended: one is fixed by a knob, the other needs a bigger machine. The
  `TrajectoryStep` carries `reasoning_tokens` / `context_used` / `context_window` so the UI shows
  both bars and names the fix.
- **Prompt nudge (D4):** a uniform reasoning-discipline block in `toolcall::prompt::build_system_for`
  (finish with the call even if reasoning is incomplete; gather info by calling a tool). A nudge,
  not a control — the load-bearing fixes are the budget + honest labeling.

The 16384 num_ctx figure is now the *fallback* ceiling; the live ceiling is the hardware-class
value above.

### File: `endstate.rs`
- **Responsibility:** Checkpoint matching + Driver-D semantic schema validation.
- **Why:** Reuses the tool-call scorer's `args_match` so the agentic success bar
  equals the single-turn bar; `validate_call` returns actionable error text the
  runner injects for recovery.
- **What:** `checkpoint_matches(&TaskCheckpoint,&Call) -> bool`,
  `validate_call(&Call,&[ToolSchema]) -> Result<(),String>` (declared tool +
  required params present + flat type match).

### File: `step.rs`
- **Responsibility:** The streamed per-turn event type.
- **What:** `StepKind::{ToolCall, ToolError, UnknownTool, SchemaError, MalformedJson,
  HallucinatedCompletion, EndStateReached, InfiniteLoop, ForbiddenCall, TurnTimeout,
  ReportedInProse, ForeignDialect, EmptyOutput, Truncated, ReasoningOverrun}`;
  `TrajectoryStep{run_index, step_index, raw_output, injection, kind, …, reasoning_tokens,
  context_used, context_window}` (the last three drive the D9 two-bar diagnostic).
  `ReportedInProse` (G3) = did all the work but answered in plain text instead of the
  required reporter tool (content-correct, wrong-channel); the UI renders it TEAL — the
  mildest failure, distinct from a hard red fail.

### File: `report.rs`
- **Responsibility:** Fold per-run outcomes into the Pass^k `AgenticReport`;
  classify/tally failure modes.
- **Why:** Distinct non-overlapping tallies so loop failures don't hide fake-done /
  bad-schema failures; effort averages only successful runs; inapplicable metrics
  are `None`, never 0.
- **What:** `FailureKind`, `RunOutcome` (`success`/`failure`/`with_schema`/`with_unknown_tools`/`with_dialect`),
  `FailureTracker{infinite_loop_hits, hallucinated_completions, malformed_json_calls,
  schema_unrecovered_calls, unknown_tool_calls, forbidden_calls, turn_timeouts,
  reported_in_prose_calls}`
  (`top() -> TopError`, `merge(&Self)` — the centralized fold that never drops a field;
  `reported_in_prose` ranks LEAST-severe in `top()`, so count-first surfaces it as a
  capable model's headline while a genuine failure dominates on a tie), `TopError`,
  `AgenticReport{passes, total_runs, failures, avg_output_tokens_success, avg_steps,
  top_error, schema_resilience, tier, requested_runs, dialect}` (`from_outcomes`;
  builders `with_tier`/`with_truncation`; `is_strict_pass()` = all-ran-passed AND not
  budget-truncated).

```rust
schema_resilience: (schema_hits > 0).then(|| schema_recovered as f64 / schema_hits as f64),
```

### File: `runner.rs`
- **Responsibility:** The Pass^k driver (`run_agentic`) + the single stateful
  attempt (`run_once`).
- **Why:** Runs `k` iterations over a fresh transcript on the shared immutable
  sandbox; iterative `while step < max_steps` (no async recursion). Fault traps
  (Driver B) fire *before* checkpoint advance so a trapped call can never be a fake
  pass; schema recovery (Driver D) injects an error and retries up to `max_recovery`.
- **Multi-call turns:** a turn processes **every** parsed call in array order (the
  system prompt invites a JSON array; `extract_calls` returns all of them) — never just
  the first. A **forbidden pre-scan** runs first: any schema-valid `must_not_call` match
  *anywhere* in the turn is terminal, so a trap can't be laundered by batching it with a
  winning move (malformed forbidden calls are exempted — they fall to schema recovery). A
  schema-invalid call corrects and burns one recovery but **does not drop** its valid
  siblings; two calls can satisfy two checkpoints in one turn. The raw turn is pushed once,
  then one tool-result line per call; the turn streams a single `TrajectoryStep` carrying
  every injection. A single-call turn is byte-identical to the pre-multi-call path.
- **Wall-clock budget (`task_budget(k, is_thinking) = PER_RUN_BUDGET(300s) × (3 if thinking) × k`):**
  a Pass^k batch over a slow model (a 12B on a 16GB host generates minutes per step) can otherwise
  grind for hours — `k × max_steps` real multi-minute turns. The budget **scales with `k`**: a flat
  per-task cap silently broke Pass^k for every tier above the smallest (a slow model takes minutes
  per run, so a flat 8-min cap let only ~3 runs through whether k was 5 or 16 — truncating the batch
  and voiding its strict-pass credit before it could finish). Per-run scaling gives each requested
  repetition a guaranteed slice, so the cap fires only on a pathologically slow batch, never merely
  because k is large. It **also scales with `is_thinking`** (×3): a reasoning model emits a
  multi-k-token `<think>` scratchpad each turn (its per-turn token budget is already 6–16× a terse
  model's), so a run is intrinsically slower and would otherwise truncate while progressing normally;
  `STEP_TIMEOUT` still caps any single wedged turn. `run_agentic_within` stops launching NEW runs once the batch
  passes the budget, but only BETWEEN whole runs and after sampling at least one, so every
  counted run is complete. The report is stamped `requested_runs = Some(k)` (truncated);
  `total_runs` holds the completed runs and the pass rate stays an honest, unbiased estimate
  over them. **Strict Pass^k veto:** `AgenticReport::is_strict_pass()` requires
  `requested_runs.is_none()` — a truncated batch can never be credited as a full pass^k (the
  un-run reps were never observed), so `passes == total_runs` on a 1-of-16 truncation isn't
  mistaken for a clean pass. `agg_agentic` (batch.rs) and the UI badge both route through it.
- **Dialect threading:** `run_steps` takes a `&mut ToolCallDialect` out-param updated at the
  `extract_calls_dialect` site; `run_once_inner` stamps it once onto the outcome via
  `with_dialect` (keeping the many terminal returns untouched). `from_outcomes` rolls up the
  first non-standard dialect onto the report → UI flag.
- **What:** `AgenticConfig{k,max_steps,max_recovery}` (Default `{5,10,2}`);
  `run_agentic(turn,sandbox,config,tx) -> AppResult<AgenticReport>`;
  `run_once(turn,sandbox,max_steps,max_recovery,run_index,tx) -> AppResult<RunOutcome>`;
  `MAX_TOKENS=256`, `STEP_TIMEOUT=180s` (per-turn wall-clock → `TurnTimeout`).

```rust
pub async fn run_agentic<M: ModelTurn>(turn, sandbox, config, tx) -> AppResult<AgenticReport> {
    let start = Instant::now();
    let mut outcomes = Vec::with_capacity(config.k as usize);
    for run_index in 0..config.k {
        if run_index > 0 && start.elapsed() >= task_budget(config.k, turn.is_thinking()) { truncated = true; break; } // whole runs only; budget scales with k AND thinking
        outcomes.push(run_once(turn, sandbox, config.max_steps, config.max_recovery, run_index, tx).await?);
    }
    let report = AgenticReport::from_outcomes(&outcomes);
    Ok(if truncated { report.with_truncation(config.k) } else { report })  // honest partial, flagged
}
```

**The step loop** (trimmed) — emit a step, parse ALL calls, forbidden-pre-scan, then
per call validate / fault-trap / advance / inject; or classify a terminal failure:

```rust
for step_index in 0..max_steps {
    let (raw, stats) = turn.run(&spec).await?;            // greedy temp 0, prompt = convo.render()
    output_tokens += stats.eval_count.unwrap_or(0);
    let calls = match extract_calls_dialect(&raw) {       // standard JSON, else normalize a native dialect
        None => return /* abstain success | malformed (broken JSON) | hallucinated yield */,
        Some((calls, d)) => { if d != Standard { *dialect = d; } calls }   // flag a normalized run
    };
    if matches!(sandbox.end_state, ExpectAbstainingText) {  // acted when it should decline
        return Ok(RunOutcome::failure(step_index+1, output_tokens, FailureKind::Hallucinated));
    }
    for call in &calls {                                  // step 1 — forbidden dominates the whole turn
        let schema_ok = sandbox.tools.is_empty() || validate_call(call, &sandbox.tools).is_ok();
        if schema_ok && sandbox.must_not_call.iter().any(|m| m.matches(call)) {
            return Ok(RunOutcome::failure(step_index+1, output_tokens, FailureKind::ForbiddenCall));
        }
    }
    let mut model_pushed = false;                         // step 2 — process every call in order
    for call in &calls {
        if let Err(msg) = validate_call(call, &sandbox.tools) {           // Driver D — no-drop
            if recoveries >= max_recovery { return MalformedSchema; }
            recoveries += 1; push_model_once(); convo.push_tool_result(&schema_err(msg)); continue;
        }
        if let Some(err) = state.fault_for(call, &sandbox.faults) {       // Driver B (before advance)
            push_model_once(); convo.push_tool_result(&err); continue;
        }
        if advance_checkpoint(call) /* RequireAll consume-once | RequireSequence ordered */ {
            return Ok(RunOutcome::success(step_index+1, output_tokens));  // race-free: forbidden pre-scanned
        }
        let result = sandbox.respond(call).unwrap_or(UNKNOWN_TOOL);       // ack | entity blob | unknown-tool
        push_model_once(); convo.push_tool_result(&result);
    }
    send_one_step_for_the_turn(/* joined injections */); // one TrajectoryStep per turn
    // loop detector: identical (canonical) calls as the previous turn AND no checkpoint
    // advanced → stall. After STALL_REPEAT_LIMIT (3) such turns in a row, fail fast as
    // InfiniteLoop instead of grinding the whole budget. A differing/advancing turn resets.
    if !progressed && this_turn_sig == prev_turn_sig { stalled_repeats += 1; } else { stalled_repeats = 0; }
    if stalled_repeats + 1 >= STALL_REPEAT_LIMIT { return Ok(RunOutcome::failure(step_index+1, …, InfiniteLoop)); }
}
// step cap exhausted → infinite loop
Ok(RunOutcome::failure(max_steps, output_tokens, FailureKind::InfiniteLoop).with_schema(hit_schema_error, schema_recovered))
```

The loop detector matters because an action ack (`{"ok":true}`) gives a model no signal
its args were wrong: a model calling the right tool with non-matching args (e.g.
`add_marker(marker:"@flaky")` when the checkpoint wants `*flaky*`) would otherwise repeat
the same turn for the full `max_steps`. The verdict is unchanged (`InfiniteLoop`), just
reached in ~3 steps. (Authoring note: v2 checkpoints should glob tolerant string args —
`marker:"*flaky*"`, not exact `"flaky"` — so a hint-following model isn't failed on a
trivial convention difference; mismatches between a scenario's world_state hints and its
checkpoint args otherwise read as model failures.)

(Authoring note — the two `world_state` reachability contracts, both CI-enforced in
`scenarios.rs`:

1. **Every entity id must be reachable.** A digit-bearing top-level `world_state` key
must appear whole-word in the prompt (a ROOT entity the model is told to act on) or
inside another top-level entry's serialized blob (a DISCOVERED entity, e.g. a wire blob
naming its counterparty). `world_state` backs only the responder and is never shown to
the model, and there is no enumeration tool — an orphaned id leaves the model no way to
know which entity to fetch, so it asks a clarifying question: a no-tool-call turn scored
`HallucinatedCompletion` on every run. Reachable ids also let `generator::instantiate`
alpha-rename consistently across prompt + world_state. Guard:
`every_world_state_entity_id_is_reachable_from_the_prompt_or_a_blob`.

2. **Every expected getter call must resolve to real data.** `derive_response` resolves
the first string arg that matches a top-level ws key (whole entity blob back), else the
blob authored under the TOOL's own name (for computation/no-arg getters like
`run_import_check{}` / `convert_temp{k:…}`), else a generic `{"ok":true}` ack. Reference
data must therefore live under top-level keys matching the getter's arg values (e.g.
`"MShop"`, `"GDPR"`, `"visa"`) — NOT nested under a wrapper map (`"policy": {"MShop": …}`
is unreachable). A getter that acks hides the fact the task grades on: the model called
the right tool, learned nothing, and springs the trap blind. Guard:
`every_expected_getter_call_resolves_to_real_world_state_data`.

3. **The world must channel the model through the answer key, honestly.** Two coding-env
conventions, learned from a live trace where a model wrote the correct fix and still
failed (`md_co_trace_root_cause`): (a) `search_*` tools return LOCATORS ("defined in
tests/test_round_paise.py"), never content — if search returns the content directly, the
model has no reason to make the `read_file{*glob*}` call the answer key checkpoints, and
scores 5/6 forever; the located path is its own ws key holding the content. (b) Stateful
status getters (`run_tests`, `run_ci`, `run_import_check`) in the STATELESS entity env
state pass/fail CONDITIONS ("FAILS while … uses float round(); GREEN once … quantizes
via Decimal"), never a bare current-state field ("failing": …) — a bare state can never
change, so a model that applied the correct fix re-runs, reads "failing", and loops until
the cap. Guard: `scripted_natural_route_completes_trace_root_cause` proves the natural
search → locate → read → fix → rerun route reaches the end state.)

---

## Folder: `inference/eval/cliff/`

A **stoppable probe** that pads a tool-call task's context with synthetic filler
across an ascending token ladder and finds the largest verified depth before
accuracy collapses. All depths are MEASURED `prompt_eval_count`, never a 4:1
estimate (the seed rate is learned per rung).

### File: `mod.rs`
- Re-exports `build_ladder, run_cliff, run_cliff_with, CliffPoint, CliffReport,
  DepthScore, TaskTrace, TraceOutput, DEFAULT_DEPTHS` and `CliffPreset, CliffSource`.

### File: `presets.rs`
- **Responsibility:** The padding source — three embedded synthetic fillers or
  user text.
- **Why:** License-clean, distinct-register filler ships in-binary so the probe
  stresses the model like real RAG context.
- **What:** `CliffPreset::{CorporatePolicy, SystemLogs, FinancialLedger}` (prose /
  structured logs / tabular CSV; each `include_str!` a `*.txt`); tagged
  `CliffSource::{Preset{preset}, Text{text}}` with `.text()`.

### File: `padding.rs`
- **Responsibility:** UTF-8-safe filler construction + needle injection at a
  fractional depth.
- **Why:** All slicing behind one boundary guard so multi-byte filler never panics.
- **What:** `safe_boundary`, `build_padding(source, target_bytes)` (cycles in 4 KB
  chunks, `CHUNK_BYTES=4096`), `inject_at_depth(padding, needle, depth)` (depth
  0.0 = front, 1.0 = back).

```rust
pub fn inject_at_depth(padding: &str, needle: &str, depth: f32) -> String {
    let frac = depth.clamp(0.0, 1.0);
    let pos = safe_boundary(padding, (padding.len() as f32 * frac) as usize);
    let mut out = String::with_capacity(padding.len() + needle.len() + 4);
    out.push_str(&padding[..pos]);
    if pos > 0 { out.push_str("\n\n"); }
    out.push_str(needle); out.push_str("\n\n"); out.push_str(&padding[pos..]);
    out
}
```

### File: `engine.rs`
- **Responsibility:** The full probe — per ascending token rung, sweep the needle
  across depths, verify measured depth, classify the collapse point.
- **Why:** Find the largest *verified* context where the task still passes,
  transparently (per-task traces) and reproducibly (greedy decode), UI-free.
- **What:** `TraceOutput`, `TaskTrace`, `DepthScore`, `CliffPoint`, `CliffReport`,
  `CliffStatus::{Broken, Collapsed, NoCliff}`; `build_ladder(max_tokens, steps)`
  (ascending, `steps.max(2)`); `run_cliff` (no cancel) and the stoppable
  `run_cliff_with(..., cancel, on_rung)`.
- **Per-task turn factory:** the engine runs over a `make_turn: Fn(&ToolTask) -> impl
  ModelTurn` seam (`run_position`→`sweep`→`probe_rung`→`run_cliff_with_factory`), so the
  prompt path reuses ONE shared `&BackendTurn` (a blanket `impl ModelTurn for &M` lets the
  factory return the shared reference) while the native path builds a fresh per-task
  `NativeToolTurn` carrying that task's tool schemas. `run_cliff`/`run_cliff_with` are thin
  `&M` wrappers over `run_cliff_with_factory`. Native's structured `tool_calls` are
  canonicalized to the same JSON the scorer parses, so padding/sweeping/scoring/classification
  are byte-identical across methods — only the turn construction differs.
- **Constants:** `BYTES_PER_TOKEN=4` (seed only), `MAX_ADJUST_ATTEMPTS=1`,
  `ADJUST_TOLERANCE=0.05`, `BASELINE_PASS=0.5`, `COLLAPSE_MARGIN=0.2`,
  `DEFAULT_DEPTHS=[0.1,0.5,0.9]`.
- **Scoring:** single-turn tasks use the full cascaded `aggregate().composite`;
  agentic tasks score on JSON well-formedness only; rung composite = the **worst**
  position's score (robust everywhere).

**Stoppable probe loop** — cancel checked before each costly rung; early-stop on a
broken baseline or the first collapse:

```rust
for (i, &target) in ladder.iter().enumerate() {
    if cancel.is_cancelled() { return Err(AppError::Inference("context-cliff probe cancelled".into())); }
    let point = probe_rung(turn, model, tasks, source_text, target, depths, &mut rate).await?;
    if cancel.is_cancelled() { return Err(AppError::Inference("…cancelled".into())); }
    on_rung(i + 1, total, &point);
    let comp = point.composite; points.push(point);
    if i == 0 {
        baseline_comp = comp;
        if comp.map_or(true, |c| c < BASELINE_PASS) { break; }           // broken baseline → stop
    } else if let (Some(b), Some(c)) = (baseline_comp, comp) {
        if c <= b - COLLAPSE_MARGIN { break; }                          // first collapse IS the cliff
    }
}
```

**Padding rebuild + verify-and-adjust** (learns the byte/token rate per rung):

```rust
let mut bytes = match *rate { Some(r) => ((target as f64) * r).round() as usize,
                              None => target as usize * BYTES_PER_TOKEN };
for attempt in 0..=MAX_ADJUST_ATTEMPTS {
    let padding = build_padding(source_text, bytes);
    let (per_depth, mean_tokens, worst, trace) = sweep(turn, model, tasks, &padding, depths).await?;
    if mean_tokens > 0 { *rate = Some(bytes as f64 / mean_tokens as f64); }
    let off = (mean_tokens as f64 - target as f64).abs() / target as f64;
    if mean_tokens == 0 || off <= ADJUST_TOLERANCE || attempt == MAX_ADJUST_ATTEMPTS { break; }
    bytes = ((bytes as f64) * (target as f64) / (mean_tokens as f64)).round() as usize;
}
```

**Classification:** `Broken` if baseline composite `< BASELINE_PASS`; `Collapsed{depth}`
on the first rung dropping `<= base - COLLAPSE_MARGIN` (carrying `largest_pass`);
else `NoCliff{tested}`.

---

## Folder: `inference/eval/readiness/`

Synthesizes a measured batch report (+ context limit + VRAM fit) into ranked
**Ready / Conditional / NotReady** verdicts against a tunable use-case profile.
Pure and Tauri-free; a MEASURED hard-gate failure blocks (NotReady), soft gates
downgrade, and a required-but-**unmeasured** input is a **Conditional caveat**
("run the missing diagnostic"), never a red block — unmeasured ≠ guessed fail.
(The `pass^k` core gate is the exception: no agentic run at all still blocks.)

### File: `mod.rs`
- Declares `inputs, profile, recommend, types, verdict, vram_fit`.

### File: `types.rs`
- **What:** `EPSILON=1e-6`;
  `CliffStatus::{NotProbed(default), NoCliff{tested}, Collapsed{depth}, Broken{tested}}`;
  `AgentPath::{PromptBased, NativeFc}`; `NativeFcStatus::{Tested{pass_k}, NotSupported}`;
  `ReadinessInputs{pass_k, avg_steps, ms_per_step, cliff, fits_in_vram, vram_pressure,
  loops, hallucinated, native_fc, path}` (`native_fc` = model-level capability for the gate;
  `path` = the row's own path for the label — deliberately decoupled);
  `Readiness::{Ready, Conditional, NotReady}`;
  `ReadinessVerdict{status, blocking, conditions, path, required_tier, cleared_tier}`;
  `ModelVerdict{model, backend, verdict, memory, avg_steps, effort, pass_k, quantization,
  cliff, by_tier, failures}` (Phase 9B: `by_tier`/`failures` for the Agent Report deep-dive,
  from the **same-path** source that path's verdict gated on — one ModelVerdict per measured path).

### File: `profile.rs`
- **Responsibility:** The tunable use-case presets verdicts are measured against.
- **What:** `ReadinessProfile{id,name, min_pass_k, max_avg_steps, max_ms_per_step,
  min_context_tokens, forbid_infinite_loop, forbid_hallucinated_completion,
  require_full_vram, require_native_fc}`; `builtins()`.

| id | min_pass_k | max_avg_steps | max_ms_per_step | forbid_loop | forbid_halluc | require_full_vram | require_native_fc |
|----|-----------|---------------|-----------------|-------------|---------------|-------------------|-------------------|
| coding-agent | 0.80 | 8.0 | 5000 | true | true | **true** | false |
| rag-assistant | 0.70 | 5.0 | 8000 | true | true | false | false |
| general-agent | 0.60 | — | — | true | false | false | false |

(`min_context_tokens` is `None` on all builtins → the cliff hard gate ships off.)

### File: `verdict.rs`
- **Responsibility:** THE gate — `assess(inputs, profile) -> ReadinessVerdict`.
- **Decision rule:** any `blocking` reason ⇒ **NotReady**; else any `conditions` ⇒
  **Conditional**; else **Ready**.
- **`path` is decoupled from the native-FC gate:** `verdict.path = i.path` (the row's own
  path, set by the caller) — **not** derived from `i.native_fc`. The `require_native_fc` gate
  reads `i.native_fc`, which carries the **model-level** native capability (identical for both
  of a column's rows). So a native-capable model's prompt-based row labels itself
  `PromptBased` yet still passes `require_native_fc` (its `native_fc` is `Tested`); only a
  genuinely native-incapable model's prompt row is blocked. Honest per-row, gate per-model.

```rust
match i.pass_k {
    None => blocking.push("pass^k not measured (no agentic runs) — cannot certify".into()),
    Some(pk) if pk < p.min_pass_k - EPSILON => blocking.push(format!("pass^k {:.2} < {:.2} required", pk, p.min_pass_k)),
    Some(_) => {}
}
if p.forbid_infinite_loop && i.loops > 0 { blocking.push(format!("loops on {} run{}", i.loops, plural(i.loops))); }
if p.forbid_hallucinated_completion && i.hallucinated > 0 { blocking.push(format!("false 'done' on {} run{}", i.hallucinated, plural(i.hallucinated))); }
if p.require_full_vram { match i.fits_in_vram {
    Some(false) => blocking.push("partial offload → severe slowdown".into()),       // MEASURED fail → block
    None        => conditions.push("memory fit not measured — set a memory cap to certify".into()), // unmeasured → caveat
    Some(true)  => {} } }
if i.vram_pressure { conditions.push("high VRAM pressure near allocation ceiling".into()); }
// soft targets → Conditional only on breach; unmeasured is silent
if let (Some(mx), Some(ms)) = (p.max_ms_per_step, i.ms_per_step) { if ms > mx { conditions.push(/* slow */); } }
if let (Some(mx), Some(s))  = (p.max_avg_steps, i.avg_steps)     { if s  > mx { conditions.push(/* inefficient */); } }
let status = if !blocking.is_empty() { Readiness::NotReady }
    else if !conditions.is_empty() { Readiness::Conditional } else { Readiness::Ready };
```

#### Readiness verdict thresholds

| Gate | Type | Triggers |
|------|------|----------|
| `pass_k` unmeasured / `< min_pass_k` | hard → NotReady | always (core gate) |
| `forbid_infinite_loop` & `loops > 0` | hard → NotReady | when profile sets it |
| `forbid_hallucinated_completion` & `hallucinated > 0` | hard → NotReady | when profile sets it |
| `require_full_vram` & `fits==false` (MEASURED) | hard → NotReady | when profile sets it |
| `require_full_vram` & fit **unmeasured** | soft → Conditional caveat | when profile sets it (e.g. llama.cpp / unified memory) |
| `require_native_fc` & `NotSupported` | hard → NotReady | when profile sets it |
| `min_context_tokens` vs cliff (`Collapsed<min` / `NoCliff<min` / `Broken`) MEASURED | hard → NotReady | when profile sets `min_context_tokens` |
| `min_context_tokens` & cliff `NotProbed` (unmeasured) | soft → Conditional caveat | when profile sets `min_context_tokens` |
| `vram_pressure` (≥ 0.85·cap) | soft → Conditional | always |
| `ms_per_step > max_ms_per_step` | soft → Conditional | when both present |
| `avg_steps > max_avg_steps` | soft → Conditional | when both present |

### File: `vram_fit.rs`
- **Responsibility:** Weights + KV-cache vs cap. Only the cache is estimated;
  weights are exact.
- **Constants:** `PRESSURE_FRACTION=0.85`, `DEFAULT_FALLBACK_CTX=8192`.
- **What:** `MemoryProfile{weights_bytes, kv_cache_bytes, total_bytes, cap_bytes,
  context_length, fits, pressure, estimated}`, `Dims{…}`, `estimate(...)`,
  `try_profile(weights, dims, num_ctx, cap)`.

```rust
pub fn estimate(weights_bytes, layers, head_count, head_count_kv, embedding_length, context_length, cap_bytes) -> MemoryProfile {
    let kv_cache_bytes = calculate_kv_cache_bytes(layers, head_count, head_count_kv, embedding_length, context_length as u64);
    let total_bytes = weights_bytes.saturating_add(kv_cache_bytes);
    let fits = total_bytes <= cap_bytes;
    let pressure = fits && cap_bytes > 0 && total_bytes as f64 >= cap_bytes as f64 * PRESSURE_FRACTION;
    MemoryProfile { weights_bytes, kv_cache_bytes, total_bytes, cap_bytes, context_length, fits, pressure, estimated: false }
}
```

### File: `inputs.rs`
- **Responsibility:** Adapter `BatchColumn -> ReadinessInputs`/verdicts. Emits **one verdict
  per MEASURED PATH** (native + prompt-based), each sourced strictly from its own pass.
- **Per-path emission (the report + publish path):** `measured_paths(col)` enumerates the
  paths a column ran (native first when `agentic_native_fc.total_runs>0`, then prompt; an
  errored/unmeasured column still yields one `(PromptBased, None)` so it never vanishes);
  `source_for_path(col, path)` is the **STRICT** same-path selector (NO `native_first_source`
  fallback — a row's metrics + tier ladder come from its own pass or are an honest gap);
  `col_native_status(col)` is the **model-level** native capability shared by both rows;
  `from_source(source, path, native_fc, …)` builds the inputs with `path` set explicitly and
  `native_fc` carrying the model-level capability (decoupled — see `verdict.rs`);
  `merged_by_tier_for_path` merges siblings strictly same-path; `verdicts_for_column(…)` ties
  it together (the shared body of `assess_readiness` and `assess_report`).
- **Legacy single-verdict helpers (kept for non-report callers + tests):**
  `native_first_source(col) -> Option<&AggAgentic>` (`agentic_native_fc.filter(total_runs>0)
  .or(agentic)`), `from_column`, `verdict_for` (error column → NotReady), `agentic_metrics`,
  `pass_k_of`, `merged_by_tier_for`. `assess_report(report, profile) -> Vec<ModelVerdict>`
  now flat-maps `verdicts_for_column` (one row per measured path).

```rust
let native = col.agentic_native_fc.as_ref().filter(|a| a.total_runs > 0);
let (source, native_fc) = match native {
    Some(n) => (Some(n), NativeFcStatus::Tested { pass_k: n.pass_k().unwrap_or(0.0) }),
    None    => (col.agentic.as_ref(), NativeFcStatus::NotSupported),
};
```

### File: `recommend.rs`
- **Responsibility:** Rank verdicts best-first + pick the winner.
- **What:** `rank(&mut [ModelVerdict])`, `recommendation(&[ModelVerdict]) -> Option<&ModelVerdict>`.
  Sort: tier desc (Ready=2/Conditional=1/NotReady=0) → `effort` asc → `avg_steps`
  asc; `None` sinks to `f64::MAX` (`total_cmp`, float-safe).

---

## Folder: `inference/eval/batch.rs`

The umbrella **batch dispatcher**: one model at a time, one task at a time, mixing
single-turn and agentic tasks, with VRAM isolation between models and durable
crash-resume.

- **Key types:** `TaskOutcome::{Single{passed,trace}, Agentic{report}, Error{message}}`;
  `CompletedUnit{model, task_id, category, outcome, is_native}` (the durable
  resumable unit); `VramGate` trait (`NoVramGate` / `OllamaVramGate` — evict prior
  model + assert VRAM cleared, `Err` halts); `BatchSink` (`task_started` /
  `agentic_turn` / `task_done`); `AggAgentic` (strict Pass^k: `tasks_passed` =
  tasks where every run passed; `pass_k() = tasks_passed/tasks_total`; `by_tier:
  Vec<TierStat>`; native-FC only: `tasks_errored` = tasks whose every run hit a
  backend `Err` — carried SEPARATELY from `tasks_total` so `pass_k` is never diluted
  by infra, making the native denominator's shrink visible not silent; `native_error_class:
  NativeErrorClass{None, InfraHost, SchemaRejected, Mixed}` distinguishes a host/infra
  crash from a native-path schema rejection so an OOM never reads as model incapability);
  `TierStat{tier, tasks_passed, tasks_total, avg_steps, failures}`
  (Phase 9B: `agg_agentic` buckets reports by tier and computes per-tier `avg_steps`
  + merged `failures` alongside the strict Pass^k — feeding the Agent Report deep-dive);
  `BatchColumn{model, backend, toolcall, agentic, agentic_native_fc, error}`;
  `BatchReport{collection_id, columns, num_ctx}`.
- **Functions:** `run_batch` (test wrapper, no gate), `run_batch_resumable`
  (the VRAM-safe resumable loop — folds `prior` units silently, runs the rest,
  streams + records), `fold_report` (repaint Matrix from completed units only,
  no execution), `run_native_fc_pass` (parallel native-FC aggregate — a task whose
  every run errors is COUNTED into `tasks_errored` + classified, never silently dropped;
  an all-errored pass still emits a column, which `inputs.rs` filters on `total_runs>0`
  so it never pollutes the verdict), `batch_summaries`, `agg_agentic`.
- **Pass selection + order (`run_passes`, batch_cmd):** the UI picks the calling method(s) —
  `RunConfig.native` (Tool-Calling) and/or `RunConfig.prompt` (Prompt-based), at least one
  (Tool-Calling is the default; `prompt` defaults true for back-compat on resumed job logs). The
  NATIVE pass runs FIRST when selected (into a `skeleton_report` column shell), emitting an
  intermediate `batch-complete` (`final:false`) ONLY if the prompt pass will follow; then the
  PROMPT pass runs **when selected** (else the report is the skeleton the native aggregates merge
  into — a native-only run), merged by model; the terminal `batch-complete` carries `final:true`.
  Native steps stream to the sink tagged `is_native` (UI shows the native trajectory separately).
  Rationale: a slow native model (e.g. gemma4-qat, ~half its turns time out at the 180s
  STEP_TIMEOUT) is watchable up front; the UI keeps `running` true until `final` so a still-empty
  cell reads "Running…", not a misleading "N/A". The Matrix renders one row PER measured pass
  (`scoreRows` `hasNative`/`hasPrompt`), so a native-only run shows no empty Prompt-based row.

**Pass^k is strict** — a task is credited only when *all k runs* succeed:

```rust
tasks_passed: reports.iter().filter(|r| r.total_runs > 0 && r.passes == r.total_runs).count() as u32,
tasks_total:  reports.len() as u32,
pub fn pass_k(&self) -> Option<f64> { (self.tasks_total > 0).then(|| self.tasks_passed as f64 / self.tasks_total as f64) }
```

**Resumable loop** — a prior unit is folded with no re-run; errors are *not*
recorded (so they re-run on resume when the backend is back):

```rust
if let Some(unit) = done.get(&(target.model.as_str(), task.id.as_str())) {
    fold_completed(unit, task, &mut single_tasks, &mut single_results, &mut agentic_reports, &mut col_error);
    continue;
}
// … run, stream through sink, then record(&unit_of(target, task, outcome, false))
```

#### Batch queue states (per `(model, task)` unit)

| State | Meaning | On resume |
|-------|---------|-----------|
| not in job log | never ran | runs now |
| `CompletedUnit` (`is_native=false`) | prompt-pass result recorded | folded silently |
| `CompletedUnit` (`is_native=true`) | native-FC pass result recorded | folded into `agentic_native_fc` |
| `Error` (not recorded) | backend was down | re-runs |
| cancelled mid-run | not recorded | re-runs |

---

## Folder: `commands/eval/`

Thin Tauri IPC skin: streams events, persists, isolates hardware. Helpers
`endpoint_for(backend)` (MLX dynamic port else default) and `traces_dir(app)` live
in `toolcall_cmd` and are imported by the other command modules.

### File: `mod.rs`
- Declares `batch_cmd, batch_payloads, eval_registry, eval_run, evals_load,
  matrix_cmd, readiness_cmd, toolcall_cmd`.

### File: `toolcall_cmd.rs`
- Commands: `get_builtin_tasks`, `list_builtin_collections`, `get_builtin_collection`,
  `run_toolcall_eval`, `load_toolcall_trace`, `trace_toolcall_task`. Validates tasks,
  runs `run_eval_traced`, caches traces best-effort (skipped when `collection_id`
  is empty — i.e. cliff/quant probes).

### File: `eval_run.rs`
- `run_eval_task` — one generic `EvalTask` by id (accumulate output, then
  `eval_score::score`). `EvalRunResult{task_id, category, passed, detail, output, token_count}`.

### File: `evals_load.rs`
- `list_evals`; resolves the evals dir (`QUANTAMIND_EVALS_DIR` → resource_dir/evals
  → dev `../docs/evals`), parses YAML, sorts by id (a malformed file fails the load).

### File: `matrix_cmd.rs`
- `run_collection_matrix` (sequential per-target `run_eval_traced`, best-effort
  trace cache, `build_matrix`, append history), `load_collection_history`.

### File: `eval_registry.rs`
- Custom-collection CRUD: `list_custom_collections`, `load_custom_collection`,
  `save_custom_collection`, `delete_custom_collection`, `read_text_capped`,
  `import_custom_collection` (size-capped read in Rust; frontend passes only paths).
  Delegates to `persistence::evals`.

### File: `batch_payloads.rs`
- Event constants `EVENT_BATCH_PROGRESS="batch-progress"`,
  `EVENT_AGENTIC_STEP="agentic-step"`, `EVENT_BATCH_COMPLETE="batch-complete"`;
  `BatchProgress::{Started{…}, Done{model,task_id,outcome}}`,
  `AgenticStepPayload{model,task_id,#[serde(flatten)] step}`, `BatchCompletePayload{report}`.

### File: `batch_cmd.rs`
- `run_batch_eval`, `stop_batch_eval`, `check_unfinished_run`, `resume_batch_eval`,
  `discard_run`. `BatchRunState{cancel}`; `TauriBatchSink` emits progress/step
  events; `OllamaVramGate` isolation; shared `run_passes` core.

```rust
// Transactional finish: persist → verify on disk → only THEN delete the job log
reports::save(&reports_d, &report)?;
if reports::load(&reports_d, &config.collection_id)?.is_none() {
    return Err(AppError::Io("batch report did not persist — keeping the resumable job log".into()));
}
let _ = queue::delete(&job_path);
```

### File: `readiness_cmd.rs`
- `run_context_cliff` / `stop_context_cliff`, `save_cliff_result` / `get_cliff_results`,
  `assess_readiness`, `list_readiness_profiles` / `save_readiness_profile` /
  `delete_readiness_profile`. Const `CLIFF_CTX_HEADROOM=2048`,
  `EVENT_CLIFF_PROGRESS="cliff-progress"`, `EVENT_CLIFF_STEP="cliff-step"`.
  `run_context_cliff` forces temp 0, `num_ctx = needed_ctx = max_tokens + 2048`, registers a cancel
  token, emits a rung event per `on_rung` AND a fine-grained `cliff-step` per
  `on_step` (one per task generation — `StepProgress{rung,position,task,...}` — so the UI
  shows movement during a slow deep rung instead of freezing between rungs), persists the
  classified `CliffStatus` only on success. **llama.cpp pre-flight:** for the
  `LlamaCpp` backend it takes the selected model's GGUF `model_path` and queries
  `LlamaServerState::probe_readiness(path)` (added there) — `NotRunning`/`WrongModel`
  (matched on the exact launch path, since the single-model server ignores the request's
  model field) → `start_with_model_msg`; `Ready{ctx}` with `ctx < needed_ctx` →
  `raise_or_reduce_msg`. Returning `Err` before the ladder means a user-managed server
  loaded with the wrong model or too small a `-c` yields one honest message instead of a
  400 on every deep rung (or a silently mis-scored model). **Ollama VRAM pre-flight
  (additive):** since Ollama sizes `num_ctx` per request, a separate branch estimates the
  deepest rung's footprint (`vram_fit::try_profile` with the device cap from `snapshot()` via
  `device_cap_bytes`) and returns `cliff_vram_msg` ("reduce Max Tokens to about N") when it
  won't fit — kept distinct from the llama.cpp identity guard (different backends fail
  differently). MLX has no readable weights/dims, so it's left to size per request.
  **Native tool-calling:** `run_native_fc: Option<bool>` selects the path — prompt (a shared
  `BackendTurn`) vs native (a per-task `NativeToolTurn` factory built via `cliff_terminal`),
  run through `run_cliff_with_factory` (see `cliff::engine`). Native is gated by
  `probe_native_tools` (reused from `batch_cmd`) — MLX / a no-tools template → refuse with
  "switch to Prompt-based". A native cliff persists under a `"{model}::native_fc"` key so it
  never clobbers the prompt-based cliff the readiness verdict reads. `assess_readiness`
  loads the persisted batch report, pulls real weights/quant from Ollama, computes
  per-column VRAM fit via `vram_fit::try_profile` (only when `cap_bytes` set),
  builds verdicts via `verdict_for`, then `recommend::rank`.

---

## Data-flow walkthroughs

### (a) Single tool-call eval scoring

1. `run_toolcall_eval(model, backend, collection_id, tasks, params)` validates via
   `tasks::validate_tasks`, resolves `endpoint_for`/options.
2. `run_eval_traced` loops `trace_one` per task: `prompt::build_system` →
   `GenerateSpec` (temp 0, `MAX_TOKENS=256`) → backend completion + `GenerateStats`.
3. `parse::extract_calls` greedily pulls + dedups calls; `score::score` produces a
   `Verdict` (length-guarded bijection on tool/args, or `abstain_correct`).
4. `eval::aggregate` folds per-task verdicts into a `ToolCallReport` with **cascaded
   conditional denominators** (parse → tool-select over parsed → arg over
   tool-matched → abstain over NoCall; composite = mean of available subs).
5. Traces cached via `eval_trace_store::upsert`; report returned to the UI.

### (b) Agentic multi-step run

1. Batch hits a `category=="agentic"` task → `build::sandbox_for` →
   `(DeterministicSandbox, AgenticConfig{k,max_steps,max_recovery})`.
2. `run_agentic` runs `run_once` `k` times on the shared immutable sandbox, fresh
   `Conversation` each iteration.
3. Per step: model emits text → `extract_calls` (ALL calls) → forbidden pre-scan
   (any schema-valid `must_not_call` anywhere in the turn → terminal) → then per call:
   Driver D (`validate_call`, inject error + burn one recovery, no-drop) → Driver B
   (`fault_for` trap *before* advance) → `checkpoint_matches` advances → `sandbox.respond`
   injects the result; the turn streams ONE `TrajectoryStep` (joined injections) to the
   `BatchSink`.
4. Terminal: `EndStateReached` (success), `Hallucinated`/`Malformed`/`MalformedSchema`/
   `ForbiddenCall`/`TurnTimeout`/`InfiniteLoop` (failure). `AgenticReport::from_outcomes`
   computes strict Pass^k
   (all-k-pass), avg steps, effort (success only), `schema_resilience`, `top_error`.

### (c) Batch run resume after crash

1. `run_batch_eval` writes a job-log header (`queue::create`) with the full config,
   then `run_passes` appends each `CompletedUnit` as it finishes (durable).
2. A crash leaves the `.jsonl` job log on disk. On next launch `check_unfinished_run`
   reports an `UnfinishedRun{run_id, done, total}`.
3. `resume_batch_eval(run_id)` loads `(config, units)`, `fold_report` repaints the
   Matrix in one `EVENT_BATCH_COMPLETE` payload, then `run_passes` continues:
   `run_batch_resumable` folds the `done` units silently (no re-run), runs only the
   remaining `(model, task)` pairs. Errors and cancellations were never recorded, so
   they re-run.
4. VRAM gate evicts the prior Ollama model and asserts VRAM cleared before the next
   loads (an `Err` halts with the log intact). On success: transactional finish
   (persist → verify on disk → delete the job log).

### (d) Context Stress Test

1. `run_context_cliff(run_id, model, backend, tasks, source, max_tokens, steps,
   params)` forces temp 0 and `num_ctx = max_tokens + 2048`, registers a cancel
   token (superseding any prior run), builds the ascending ladder (`build_ladder`).
2. `run_cliff_with` walks rungs ascending: `probe_rung` builds padding sized by the
   learned byte/token rate, sweeps the needle across `DEFAULT_DEPTHS=[0.1,0.5,0.9]`
   (`inject_at_depth`), measures real `prompt_eval_count`, and verify-adjusts once if
   off by `> ADJUST_TOLERANCE`. Rung composite = the **worst** depth's score.
3. After each rung, `on_rung` emits `EVENT_CLIFF_PROGRESS`; within a rung, `on_step`
   fires per task generation (threaded `run_cliff_with`→`probe_rung`→`sweep`→`run_position`)
   and emits `EVENT_CLIFF_STEP` so the panel's bar/ETA advance mid-rung. Early-stop: a baseline
   `< BASELINE_PASS` ⇒ `Broken` (break); the first rung dropping `<= base -
   COLLAPSE_MARGIN` ⇒ `Collapsed{depth}` (break). `cancel` is checked before each
   rung, after `probe_rung`, and before classify/persist.
4. `classify` produces `CliffStatus::{Broken, Collapsed, NoCliff}` + `cliff_tokens`
   (largest verified pass). Persisted only on a non-cancelled success; later read by
   `assess_readiness` as the `cliff` input to the readiness verdict.
