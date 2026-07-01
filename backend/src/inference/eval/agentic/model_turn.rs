use crate::errors::AppResult;
use crate::inference::backend::backend::InferenceBackend;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::chat::chat_templates::detect_template;
use crate::inference::eval::agentic::difficulty::passk::NON_THINKING_MAX_TOKENS;
use crate::inference::eval::toolcall::prompt::{terminal_closing, TerminalGuidance};
use crate::inference::eval::toolcall::tasks::ToolSchema;
use crate::inference::generate::generate_options::GenerateOptions;
use crate::inference::generate::generate_spec::GenerateSpec;
use crate::inference::generate::generate_stats::GenerateStats;
use crate::inference::llama::llama_backend::LlamaCppBackend;
use crate::inference::llama::llama_chat;
use crate::inference::mlx::mlx_backend::MlxBackend;
use crate::inference::ollama::ollama_backend::OllamaBackend;
use crate::inference::ollama::ollama_chat::{self, NativeToolCall};
use crate::inference::ollama::ollama_show::show_model;
use serde_json::{json, Value};
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;

/// Resolve the stop tokens for a model so generation actually halts. The end-of-turn
/// markers of harmony (`<|return|>`/`<|call|>`) and gemma (`<end_of_turn>`) aren't a plain
/// EOS, so without them the model emits the markers as literal text and runs to the token
/// cap (the infinite-generation bug). The architecture comes from Ollama `/api/show`
/// `model_info["general.architecture"]` — a METADATA-only call that does NOT load/offload
/// weights, so it adds no model-switch latency — then the chat-template table maps it to its
/// stops. Any failure (non-Ollama backend, Ollama down, unknown family) degrades to `[]`
/// (the prior no-stop behavior), never an error. Called once per turn and memoized.
async fn resolve_model_stops(endpoint: &str, backend: BackendKind, model: &str) -> Vec<String> {
    // Scoped to Ollama (the failing path); llama.cpp/MLX resolve stops on their own wire
    // structs as a follow-up.
    if backend != BackendKind::Ollama {
        return Vec::new();
    }
    let arch = show_model(endpoint, model)
        .await
        .ok()
        .and_then(|r| r.model_info.get("general.architecture").and_then(|v| v.as_str()).map(str::to_string));
    detect_template(model, arch.as_deref())
        .map(|t| t.stop_tokens.iter().map(|s| (*s).to_string()).collect())
        .unwrap_or_default()
}

/// One model turn behind a seam: prompt in → (text, stats) out. The runner
/// depends on this, not on a concrete backend, so it stays unit-testable with a
/// scripted model while the real path drives a live `InferenceBackend`.
#[allow(async_fn_in_trait)]
pub trait ModelTurn {
    async fn run(&self, spec: &GenerateSpec) -> AppResult<(String, GenerateStats)>;

    /// Best-effort: load the model resident BEFORE the first scored turn so its
    /// cold-load latency (weights into VRAM) isn't charged to the first task as a
    /// `TurnTimeout` — which would systematically penalize every model's first task and
    /// corrupt cross-model comparison. Default no-op: scripted test models need no
    /// warming, and a backend that can't warm simply runs cold (the prior behavior).
    async fn warm_up(&self) -> AppResult<()> {
        Ok(())
    }

    /// Is this a reasoning model that emits a `<think>…</think>` scratchpad before its
    /// tool call? When true the runner (a) raises the per-turn `num_predict` to
    /// [`Self::max_output_tokens`] so the model isn't truncated mid-thought, and (b) strips
    /// `<think>` from the output before parsing AND before the transcript append. Default
    /// `false`: a scripted test model and the native-FC path keep the terse-model behavior.
    fn is_thinking(&self) -> bool {
        false
    }

    /// The per-turn output-token budget (`num_predict`) the runner pins on the spec.
    /// Default is the legacy 256 cap; a thinking model returns a tier-scaled budget that
    /// clears the scratchpad range so the call survives. See
    /// `difficulty::passk::max_tokens_for`.
    fn max_output_tokens(&self) -> u32 {
        NON_THINKING_MAX_TOKENS
    }
}

/// Real path: dispatch by `BackendKind` (the trait isn't object-safe), accumulate
/// tokens into a `String`, return text + stats. Mirrors
/// `toolcall::eval::generate_text`; shares one `CancellationToken` so a stop
/// request aborts the in-flight generation.
pub struct BackendTurn {
    pub backend: BackendKind,
    pub endpoint: String,
    pub model: String,
    pub cancel: CancellationToken,
    /// Global inference params (from the header) applied to every eval turn.
    /// `None` runs at backend defaults.
    pub options: Option<GenerateOptions>,
    /// Ollama keep_alive (from the header's "keep model loaded" toggle).
    pub keep_alive: Option<i32>,
    /// This model is a reasoning model (the sidebar "thinking" checkbox). Drives the
    /// raised token budget + `<think>` stripping in the runner.
    pub is_thinking: bool,
    /// The per-turn `num_predict` for this model: tier-scaled when `is_thinking`, else 256.
    /// Precomputed at construction (`difficulty::passk::max_tokens_for`), where the tier is
    /// known, so the runner doesn't need the tier threaded in.
    pub max_tokens: u32,
    /// Per-turn-instance memo of the resolved stop tokens (see `resolve_model_stops`).
    /// Resolved lazily on the first `run` and reused for every subsequent turn of this
    /// model, so the agentic loop pays at most one `/api/show` per run. A `BackendTurn` is
    /// built fresh per eval run, so a mid-session re-import can't leave a stale mapping.
    /// Defaulted at every construction site — not a user-supplied value.
    #[doc(hidden)]
    pub stop_cache: OnceCell<Vec<String>>,
}

/// Merge the header's global eval params (`global`) with the harness's per-turn spec
/// (`spec`). Structural caps the eval loop sets — `num_predict` (the anti-runaway token
/// cap) and `num_ctx` (sized to keep the prefix-KV cache alive) — take precedence so a
/// header that leaves `max_tokens`/`num_ctx` unset can't strip them. Every other field is
/// a user sampling preference and comes from the header, falling back to the spec. `None`
/// only when neither side set any options.
fn merge_eval_options(
    global: Option<&GenerateOptions>,
    spec: Option<&GenerateOptions>,
) -> Option<GenerateOptions> {
    match (global, spec) {
        (None, s) => s.cloned(),
        (Some(g), None) => Some(g.clone()),
        (Some(g), Some(s)) => Some(GenerateOptions {
            // Harness-owned: the spec's value wins (it's a correctness/safety bound), but
            // honor a header-supplied value when the spec didn't pin one.
            num_predict: s.num_predict.or(g.num_predict),
            num_ctx: s.num_ctx.or(g.num_ctx),
            // User sampling prefs: header wins, spec is the fallback default.
            temperature: g.temperature.or(s.temperature),
            top_p: g.top_p.or(s.top_p),
            top_k: g.top_k.or(s.top_k),
            repeat_penalty: g.repeat_penalty.or(s.repeat_penalty),
            seed: g.seed.or(s.seed),
            // Stop tokens are injected per-model in `run` after this merge; carry any
            // explicitly-set value through (header wins) so the injection only fills a gap.
            stop: g.stop.clone().or_else(|| s.stop.clone()),
        }),
    }
}

impl ModelTurn for BackendTurn {
    async fn run(&self, spec: &GenerateSpec) -> AppResult<(String, GenerateStats)> {
        let mut out = String::new();
        let push = |t: &str| out.push_str(t);
        let cancel = self.cancel.clone();
        // The agentic loop builds its spec without a model name (it only knows the
        // `ModelTurn` seam). Inject our own so Ollama — which sends `spec.model` in
        // the request — targets the right model instead of an empty name. Merge the
        // global eval params with the harness spec FIELD-WISE (see `merge_eval_options`):
        // the loop's structural caps (`num_predict`, `num_ctx`) must win, or a header that
        // omits `max_tokens` strips the per-turn cap → runaway generation (minutes/turn,
        // KV-cache busting). User sampling prefs (top_p/top_k/penalty/seed/temperature)
        // still come from the header.
        // Resolve this model's stop tokens once (memoized) and fill them in if nothing
        // upstream set `stop`. This is what halts harmony/gemma models — without it they
        // emit their turn markers as text and run to the token cap. Empty for unknown
        // families ⇒ no `stop` key ⇒ identical to the prior behavior.
        let stops = self
            .stop_cache
            .get_or_init(|| resolve_model_stops(&self.endpoint, self.backend, &self.model))
            .await;
        let mut options = merge_eval_options(self.options.as_ref(), spec.options.as_ref());
        if !stops.is_empty() {
            let opts = options.get_or_insert_with(GenerateOptions::default);
            if opts.stop.is_none() {
                opts.stop = Some(stops.clone());
            }
        }
        let spec = GenerateSpec {
            model: self.model.clone(),
            options,
            keep_alive: self.keep_alive.or(spec.keep_alive),
            ..spec.clone()
        };
        let stats = match self.backend {
            BackendKind::Ollama => OllamaBackend::new(self.endpoint.clone()).generate(&spec, cancel, push).await?,
            BackendKind::LlamaCpp => LlamaCppBackend::new(self.endpoint.clone()).generate(&spec, cancel, push).await?,
            BackendKind::Mlx => MlxBackend::new(self.endpoint.clone(), self.model.clone()).generate(&spec, cancel, push).await?,
        };
        Ok((out, stats))
    }

    fn is_thinking(&self) -> bool {
        self.is_thinking
    }

    fn max_output_tokens(&self) -> u32 {
        self.max_tokens
    }

    /// Issue a 1-token generation to force the model resident (honoring `keep_alive` so
    /// it stays loaded across this model's tasks). The output is discarded; only the
    /// load side-effect matters. Bypasses the global eval `options` so warming is cheap.
    async fn warm_up(&self) -> AppResult<()> {
        let spec = GenerateSpec {
            model: self.model.clone(),
            prompt: "ok".into(),
            system: None,
            options: Some(GenerateOptions { num_predict: Some(1), temperature: Some(0.0), ..Default::default() }),
            keep_alive: self.keep_alive,
        };
        let cancel = self.cancel.clone();
        let sink = |_: &str| {};
        match self.backend {
            BackendKind::Ollama => OllamaBackend::new(self.endpoint.clone()).generate(&spec, cancel, sink).await?,
            BackendKind::LlamaCpp => LlamaCppBackend::new(self.endpoint.clone()).generate(&spec, cancel, sink).await?,
            BackendKind::Mlx => MlxBackend::new(self.endpoint.clone(), self.model.clone()).generate(&spec, cancel, sink).await?,
        };
        Ok(())
    }
}

/// The native path's system prompt. Tools are provided natively (no "respond with JSON"
/// instructions), but it carries the SAME act-vs-abstain reporter-tool mandate the prompt path
/// uses (`terminal_closing`) — without it, the old fixed "...otherwise reply in plain text"
/// nudged capable native models (gemma4) OFF the `reply` tool into prose → an unfair
/// `ReportedInProse` the prompt path doesn't suffer. Path-fairness, one source of truth.
fn native_system(tools: &[ToolSchema], terminal: TerminalGuidance) -> String {
    format!("You complete the task using the available tools. {}", terminal_closing(tools, terminal))
}

/// Native path: call the running backend's native tool API with a real `tools`
/// array and translate the structured `tool_calls` back into the canonical
/// `{"name","args"}` JSON the runner's `extract_calls` already parses — so the
/// sandbox, scoring, and `TrajectoryStep` stay byte-identical to the prompt path
/// AND across backends. Built per task (it carries that task's tool schemas +
/// the act/abstain terminal guidance).
///
/// The backend is the only dispatch point: Ollama → `/api/chat`, llama.cpp →
/// OpenAI `/v1/chat/completions` (needs `--jinja`). A new server plugs in by
/// adding one match arm in `run` + its `chat_with_tools`. MLX has no native tool
/// API and is gated out upstream (`probe_native_tools`).
pub struct NativeToolTurn {
    pub backend: BackendKind,
    pub endpoint: String,
    pub model: String,
    pub tools: Vec<ToolSchema>,
    pub options: Option<GenerateOptions>,
    /// Act-vs-abstain mandate for this task (from its end_state), so the native system tells the
    /// model to use the reporter tool on an ACT task instead of inviting prose.
    pub terminal: TerminalGuidance,
    /// Per-turn output-token budget — tier-scaled exactly like the prompt path's `BackendTurn`
    /// (`max_tokens_for(tier, is_thinking)`), so native isn't run on a different budget than
    /// prompt (budget parity across the two passes).
    pub max_tokens: u32,
    /// Reasoning model? Raises the per-turn budget so a `<think>` scratchpad isn't truncated —
    /// matches the prompt path.
    pub is_thinking: bool,
}

/// Shape the tool schemas into Ollama's `tools` array (OpenAI-style function specs).
fn build_tools_value(tools: &[ToolSchema]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|t| json!({ "type": "function", "function": { "name": t.name, "description": t.description, "parameters": t.parameters } }))
            .collect(),
    )
}

/// Translate native tool calls into the canonical JSON text the runner parses.
/// Strict serialization (`serde_json::to_string`), never interpolation, so
/// embedded quotes survive. ALL calls are synthesized into the array; the runner's
/// existing `extract_calls(...).next()` takes the first (identical to a prompt
/// model emitting several objects). Empty → `""` (pure-abstain): `extract_calls`
/// returns `None` and `looks_like_broken_json` is false → the runner's no-call arm.
fn synthesize_calls(calls: &[NativeToolCall]) -> String {
    if calls.is_empty() {
        return String::new();
    }
    let arr: Vec<Value> = calls.iter().map(|c| json!({ "name": c.name, "args": c.args })).collect();
    serde_json::to_string(&Value::Array(arr)).unwrap_or_default()
}

/// The text the runner sees from a native turn. With real `tool_calls`, the canonical JSON
/// (so scoring is byte-identical to the prompt path). With NONE, the raw assistant `content`
/// rather than `""` — see the rationale on `NativeToolTurn::run`. Pure, so the selection is
/// unit-tested without a live server.
fn native_turn_text(calls: &[NativeToolCall], content: String) -> String {
    if calls.is_empty() {
        content
    } else {
        synthesize_calls(calls)
    }
}

impl ModelTurn for NativeToolTurn {
    async fn run(&self, spec: &GenerateSpec) -> AppResult<(String, GenerateStats)> {
        let tools = build_tools_value(&self.tools);
        let system = native_system(&self.tools, self.terminal);
        // Budget parity with the prompt path: the runner pins the tier-scaled per-turn
        // `num_predict` on `spec.options` (from `max_output_tokens()` below); merge it in so the
        // native turn runs on the SAME budget — the spec's structural cap wins, user sampling
        // prefs come from `self.options` (see `merge_eval_options`).
        let options = merge_eval_options(self.options.as_ref(), spec.options.as_ref());
        // The ONLY backend dispatch: each native server has its own tool wire, but all return
        // the shared `ChatResult` so canonicalization below stays identical across backends.
        let result = match self.backend {
            BackendKind::Ollama => {
                ollama_chat::chat_with_tools(&self.endpoint, &self.model, &system, &spec.prompt, &tools, options).await?
            }
            BackendKind::LlamaCpp => {
                llama_chat::chat_with_tools(&self.endpoint, &self.model, &system, &spec.prompt, &tools, options).await?
            }
            BackendKind::Mlx => {
                return Err(crate::errors::AppError::Inference(
                    "MLX has no native tool-calling API; it must run the prompt path".into(),
                ))
            }
        };
        // When the native parser returned tool calls, hand the runner the canonical
        // JSON. When it returned NONE, surface the raw assistant `content` instead of an
        // empty string: a mis-built model often emits a foreign dialect (channel-token soup)
        // the backend can't parse into `tool_calls` but leaves in `content`; dropping it made
        // every such turn a silent empty → `Hallucinated`, hiding the real cause. Returning
        // `content` lets the runner name the honest verdict (`ForeignDialect` / prose /
        // hallucination). Parity-safe: the backend already yielded zero calls, and the forms
        // that land here (paren / `<|"|>`-wrapped soup, or plain prose) are exactly the ones the
        // text salvager (`harmony_calls`) also drops — so this never credits a call the backend
        // missed, it only makes the FAILURE honest.
        Ok((native_turn_text(&result.tool_calls, result.content), result.stats))
    }

    fn is_thinking(&self) -> bool {
        self.is_thinking
    }

    fn max_output_tokens(&self) -> u32 {
        self.max_tokens
    }
}

#[cfg(test)]
mod tests {
    use super::{merge_eval_options, native_system, native_turn_text, synthesize_calls, NativeToolCall};
    use crate::inference::eval::toolcall::parse::{extract_calls, extract_calls_dialect, looks_like_broken_json, looks_like_foreign_dialect, ToolCallDialect};
    use crate::inference::eval::toolcall::prompt::TerminalGuidance;
    use crate::inference::eval::toolcall::tasks::ToolSchema;
    use crate::inference::generate::generate_options::{GenerateOptions, EVAL_REPEAT_PENALTY};
    use serde_json::json;

    fn tool(name: &str, props: serde_json::Value) -> ToolSchema {
        ToolSchema { name: name.into(), description: format!("tool {name}"), parameters: json!({ "type": "object", "properties": props }) }
    }

    #[test]
    fn native_system_mandates_the_reporter_tool_on_an_act_task() {
        // The Gap A' fix: an ACT task with a `reply` tool must tell the native model to CALL
        // reply, not "answer in plain text" (the old fixed prompt's prose nudge → ReportedInProse).
        let tools = vec![tool("read_file", json!({ "path": { "type": "string" } })), tool("reply", json!({ "text": { "type": "string" } }))];
        let sys = native_system(&tools, TerminalGuidance::MustUseTools);
        assert!(sys.contains("call the `reply` tool"), "{sys}");
        assert!(sys.contains("Do not answer in plain text"), "{sys}");
        // Native uses real tools — never the prompt path's JSON-format instructions.
        assert!(!sys.contains("respond with ONLY a JSON object"), "{sys}");
        assert!(!sys.to_lowercase().contains("json array of such objects"), "{sys}");
    }

    #[test]
    fn native_system_keeps_plain_text_for_an_abstain_task() {
        let tools = vec![tool("search", json!({ "q": { "type": "string" } }))];
        let sys = native_system(&tools, TerminalGuidance::PlainTextOk);
        assert!(sys.contains("just answer the user in plain text"), "{sys}");
        assert!(!sys.contains("Do not answer in plain text"), "{sys}");
    }

    #[test]
    fn a_header_without_max_tokens_cannot_strip_the_harness_token_cap() {
        // The regression: a header that sets only temperature (num_predict/num_ctx unset)
        // used to REPLACE the spec wholesale → no token cap → runaway generation. The merge
        // must keep the spec's `num_predict`/`num_ctx` while taking the header's temperature.
        let global = GenerateOptions { temperature: Some(0.7), ..Default::default() };
        let spec = GenerateOptions { temperature: Some(0.0), num_predict: Some(256), num_ctx: Some(4096), ..Default::default() };
        let merged = merge_eval_options(Some(&global), Some(&spec)).unwrap();
        assert_eq!(merged.num_predict, Some(256), "the per-turn cap survives");
        assert_eq!(merged.num_ctx, Some(4096), "the sized context window survives");
        assert_eq!(merged.temperature, Some(0.7), "the header's sampling pref still applies");
    }

    #[test]
    fn header_sampling_prefs_pass_through_and_spec_only_fields_are_kept() {
        let global = GenerateOptions { top_p: Some(0.9), top_k: Some(40), seed: Some(7), ..Default::default() };
        let spec = GenerateOptions { temperature: Some(0.0), num_predict: Some(256), ..Default::default() };
        let merged = merge_eval_options(Some(&global), Some(&spec)).unwrap();
        assert_eq!(merged.top_p, Some(0.9));
        assert_eq!(merged.top_k, Some(40));
        assert_eq!(merged.seed, Some(7));
        assert_eq!(merged.num_predict, Some(256)); // spec-only field retained
        assert_eq!(merged.temperature, Some(0.0)); // header didn't set it → spec default
    }

    #[test]
    fn a_header_max_tokens_is_honored_only_when_the_spec_did_not_pin_one() {
        let global = GenerateOptions { num_predict: Some(1000), ..Default::default() };
        // spec pins the cap → spec wins (anti-runaway).
        let pinned = merge_eval_options(Some(&global), Some(&GenerateOptions { num_predict: Some(256), ..Default::default() })).unwrap();
        assert_eq!(pinned.num_predict, Some(256));
        // spec leaves it open → header value flows through.
        let open = merge_eval_options(Some(&global), Some(&GenerateOptions::default())).unwrap();
        assert_eq!(open.num_predict, Some(1000));
    }

    #[test]
    fn the_harness_repeat_penalty_default_survives_a_silent_header_but_yields_to_one_set() {
        // The anti-collapse default: the eval spec carries EVAL_REPEAT_PENALTY so a
        // greedy run can't loop to the token cap. A header that doesn't touch the
        // slider must NOT erase it; a header that does set it wins (user override).
        let spec = GenerateOptions { temperature: Some(0.0), repeat_penalty: Some(EVAL_REPEAT_PENALTY), num_predict: Some(256), ..Default::default() };
        let silent = merge_eval_options(Some(&GenerateOptions::default()), Some(&spec)).unwrap();
        assert_eq!(silent.repeat_penalty, Some(EVAL_REPEAT_PENALTY), "header silent → harness default applies");
        let override_global = GenerateOptions { repeat_penalty: Some(1.3), ..Default::default() };
        let overridden = merge_eval_options(Some(&override_global), Some(&spec)).unwrap();
        assert_eq!(overridden.repeat_penalty, Some(1.3), "header value wins over the spec default");
    }

    #[tokio::test]
    #[ignore = "hits a live Ollama on :11434 with gpt-oss / gemma4 installed"]
    async fn live_resolve_stops_maps_installed_models_to_their_real_stop_tokens() {
        use super::{resolve_model_stops, BackendKind};
        let ep = "http://localhost:11434";
        // End-to-end: /api/show arch → chat-template stops, for the models that loop.
        assert_eq!(
            resolve_model_stops(ep, BackendKind::Ollama, "gpt-oss-20b_q8_0:latest").await,
            vec!["<|return|>".to_string(), "<|call|>".to_string()],
        );
        assert_eq!(
            resolve_model_stops(ep, BackendKind::Ollama, "gemma-4-12b-it-qat_q4_0:latest").await,
            vec!["<end_of_turn>".to_string()],
        );
        // Non-Ollama backends short-circuit to no stops without any network call.
        assert!(resolve_model_stops(ep, BackendKind::Mlx, "anything").await.is_empty());
    }

    #[test]
    fn an_explicit_stop_is_carried_through_the_merge_for_run_to_respect() {
        // run() only fills `stop` when it's still None after the merge, so an explicitly
        // set value must survive (header wins, then spec).
        let global = GenerateOptions { stop: Some(vec!["X".into()]), ..Default::default() };
        let spec = GenerateOptions { stop: Some(vec!["Y".into()]), num_predict: Some(256), ..Default::default() };
        assert_eq!(merge_eval_options(Some(&global), Some(&spec)).unwrap().stop, Some(vec!["X".into()]));
        let spec_only = GenerateOptions { stop: Some(vec!["Y".into()]), ..Default::default() };
        assert_eq!(merge_eval_options(Some(&GenerateOptions::default()), Some(&spec_only)).unwrap().stop, Some(vec!["Y".into()]));
        // Neither side set it → None, so run() is free to inject the model's resolved stops.
        let bare = GenerateOptions { num_predict: Some(256), ..Default::default() };
        assert_eq!(merge_eval_options(Some(&GenerateOptions::default()), Some(&bare)).unwrap().stop, None);
    }

    #[test]
    fn missing_sides_degrade_gracefully() {
        let spec = GenerateOptions { num_predict: Some(256), ..Default::default() };
        assert_eq!(merge_eval_options(None, Some(&spec)).unwrap().num_predict, Some(256));
        let global = GenerateOptions { temperature: Some(0.5), ..Default::default() };
        assert_eq!(merge_eval_options(Some(&global), None).unwrap().temperature, Some(0.5));
        assert!(merge_eval_options(None, None).is_none());
    }

    #[test]
    fn single_call_round_trips_through_extract_calls_with_embedded_quotes() {
        let calls = vec![NativeToolCall { name: "get_weather".into(), args: json!({ "city": "Paris \"Île\"" }) }];
        let parsed = extract_calls(&synthesize_calls(&calls)).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "get_weather");
        assert_eq!(parsed[0].args, json!({ "city": "Paris \"Île\"" }));
    }

    #[test]
    fn abstain_yields_empty_text_classified_as_a_clean_no_call() {
        let text = synthesize_calls(&[]);
        assert_eq!(text, "");
        assert!(extract_calls(&text).is_none());
        assert!(!looks_like_broken_json(&text)); // not a MalformedJson — a true abstain
    }

    #[test]
    fn native_turn_with_calls_returns_canonical_json_ignoring_content() {
        // When Ollama parsed real tool calls, the content is irrelevant — the runner scores
        // the canonical JSON, byte-identical to the prompt path.
        let calls = vec![NativeToolCall { name: "run_tests".into(), args: json!({ "module": "cart" }) }];
        let text = native_turn_text(&calls, "some prose Ollama also returned".into());
        let parsed = extract_calls(&text).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "run_tests");
    }

    #[test]
    fn native_turn_with_no_calls_surfaces_foreign_content_so_the_runner_flags_it() {
        // The bug: Ollama's parser found NO tool_calls in a mis-built model's channel-token
        // soup, but the soup is in `content`. Returning "" hid it as a silent empty →
        // Hallucinated. Surfacing `content` lets the runner name the honest ForeignDialect.
        let soup =
            "<channel|><|tool_response|>call:reply(text='cart suite failed: test_apply_discount_negative_total')<tool_call|>";
        let text = native_turn_text(&[], soup.into());
        assert_eq!(text, soup);
        // Parity-safe: the soup is NOT salvaged (no call credited), but it IS flagged foreign.
        assert!(extract_calls_dialect(&text).is_none());
        assert!(looks_like_foreign_dialect(&text));
    }

    #[test]
    fn native_turn_with_no_calls_and_clean_harmony_brace_matches_what_ollama_recovers() {
        // The clean `call:NAME{…}` form Ollama's native parser DOES recover would normally
        // arrive as a real tool_call; if it ever reaches the content branch the salvager
        // recovers it as Harmony — same call a real deployment gets (production parity).
        let text = native_turn_text(&[], "<channel|>call:run_tests{module: \"cart\"}<tool_call|>".into());
        let (calls, dialect) = extract_calls_dialect(&text).unwrap();
        assert_eq!(dialect, ToolCallDialect::Harmony);
        assert_eq!(calls[0].name, "run_tests");
    }

    #[test]
    fn native_turn_with_no_calls_and_empty_content_is_still_a_clean_abstain() {
        // A genuine native abstention (no calls, no content) stays an empty no-call yield.
        let text = native_turn_text(&[], String::new());
        assert_eq!(text, "");
        assert!(extract_calls(&text).is_none());
        assert!(!looks_like_foreign_dialect(&text));
    }

    #[test]
    fn parallel_calls_synthesize_all_and_the_runner_takes_the_first() {
        let calls = vec![
            NativeToolCall { name: "a".into(), args: json!({ "x": 1 }) },
            NativeToolCall { name: "b".into(), args: json!({ "y": 2 }) },
            NativeToolCall { name: "c".into(), args: json!({}) },
        ];
        let parsed = extract_calls(&synthesize_calls(&calls)).unwrap();
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed.into_iter().next().unwrap().name, "a");
    }
}
