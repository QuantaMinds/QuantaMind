# Backend — Inference Engine Abstraction & Model Servers

The Rust subsystem that turns *"generate text from model M"* into a streamed
token feed, regardless of which of three local engines actually runs the
weights. This document covers the `InferenceBackend` trait, its three
implementations (llama.cpp, vLLM), the shared HTTP/NDJSON/SSE plumbing,
the wire/stats codecs, and the per-engine server-process lifecycle.

> Cross-links:
> - Model listing, HF discovery, GGUF inspection, and pulls live in
>   **`backend-models-hf-gguf.md`**.
> - The single-prompt Tauri command that drives a generation is in
>   **`backend-prompt-workspace-system.md`** (`run_prompt`).
> - The compare grid that fans this out across models is in

---

## Overview

### Why an abstraction over three engines

QuantaMind runs models through three servers — one local, two remote:

| Engine | Weights | Process |
|---|---|---|
| **llama.cpp** | a single `.gguf` file | bundled `llama-server` sidecar (port 8081) |
| **vLLM** | whatever the remote box serves | user-run, user-configured endpoint |
| **vLLM** | whatever the remote box serves | user-run, user-configured endpoint |

All three speak the OpenAI wire, but they report *different stats* (llama.cpp gives
prompt/predict ms plus a prompt-cache count; the remote servers give token counts
only), and have a *different process lifecycle* (llama.cpp is an app-spawned child;
the remote servers are never spawned or reaped by us). The rest of the app —
the single-prompt command, the compare grid, the eval engine — must not care
about any of that. So everything funnels through one trait.

### What the trait guarantees

`InferenceBackend::generate` is the *entire* contract: given a `GenerateSpec`
(model, prompt, system, options, keep-alive) and a `CancellationToken`, stream
each token of response text through an `on_token(&str)` closure, and return a
normalized `GenerateStats` when the model stops — or `GenerateStats::default()`
(all-`None`) if `cancel` fires. Callers stay backend-agnostic and pick an
implementation with a `match backend { … }`.

**Backend selection is absolute, never a health fallback.** A model carries its
`BackendKind` (from `ModelInfo.backend`), decided by its *weight format* at
discovery time — a `.gguf` is `LlamaCpp`; a remote-served id is `VLlm`/`VLlm`
registry entry is `llama.cpp`. The dispatch (`run_prompt_inner`) matches on that
field and *only* that field. It never tries another engine because one looks
healthier; an vLLM model is never served by llama-server even if llama's
`/health` happens to answer. (Robustness fallbacks exist *within* one backend —
e.g. llama's `/completion` → `/v1/chat/completions` — but never *across* the
`BackendKind` boundary.)

### How a generate request flows

```
UI (React)
  └─ invoke("run_prompt", { backend, model, prompt, options, … })
       └─ commands/prompt/prompt.rs            // thin Tauri command
            • pick endpoint:  VLlm → remote_config::vllm() (user-configured)
                              else → endpoint::default_for(backend)
            • wrap emit in make_token_handler (counts tokens, cancels on emit-fail)
            └─ run_prompt_inner(backend, endpoint, …)   // prompt_run.rs
                 match backend {
                   llama.cpp   => llama.cppBackend::new(ep).generate(&spec, …)
                   LlamaCpp => LlamaCppBackend::new(ep).generate(&spec, …)
                   VLlm     => VLlmBackend::new(ep, key, model).generate(&spec, …)
                 }
                   └─ <Engine>Backend::generate          // *_backend.rs (trait impl)
                        └─ stream_generate(endpoint, …)   // *.rs (HTTP + wire codec)
                             • POST request body  (engine-specific *_wire.rs)
                             • streaming_client()  (http/http.rs)
                             • loop: read bytes → next_line (http/ndjson.rs)
                                     → strip_sse → parse chunk (*_chunk / *_wire)
                                     → on_token(text)   ── token events back to UI
                             • on stop chunk → GenerateStats  (*_timings / *_stats)
```

The dispatch site (`prompt_run.rs`, trimmed):

```rust
match backend {
    BackendKind::llama.cpp =>
        llama.cppBackend::new(endpoint.to_string()).generate(&spec, cancel, on_token).await,
    BackendKind::LlamaCpp =>
        LlamaCppBackend::new(endpoint.to_string()).generate(&spec, cancel, on_token).await,
    BackendKind::VLlm =>
        VLlmBackend::new(endpoint.to_string(), model.to_string())
            .generate(&spec, cancel, on_token).await,
}
```

---

## `inference/backend/` — the trait & engine identity

#### File: `inference/backend/backend.rs`
- **Responsibility:** Define the one streaming-generation contract.
- **Why:** A single async method lets the prompt/compare/eval layers treat all
  three engines identically and select via a `BackendKind` match.
- **What:** `trait InferenceBackend` with one method, `generate<F: FnMut(&str)>`.
- **How/Where used:** Implemented by `llama.cppBackend`, `LlamaCppBackend`,
  `VLlmBackend`; called from `commands/prompt/prompt_run.rs` and the
  compare/eval paths.

```rust
#[allow(async_fn_in_trait)]
pub trait InferenceBackend {
    async fn generate<F: FnMut(&str)>(
        &self,
        spec: &GenerateSpec,
        cancel: CancellationToken,
        on_token: F,
    ) -> AppResult<GenerateStats>;
}
```

#### File: `inference/backend/backend_kind.rs`
- **Responsibility:** The closed set of engines a model can be served by.
- **Why:** Surfaces over IPC as `ModelInfo.backend` and is the *only* selector
  for dispatch — backend identity is a property of the model, not a runtime choice.
- **What:** `enum BackendKind { LlamaCpp (default), VLlm }`,
  `#[serde(rename_all = "snake_case")]` — with per-variant `#[serde(rename)]` on the
  last two so they round-trip to TS as `"llama_cpp" | "llama_cpp" | "vllm" | "vllm" |
  "vllm"` (not `"v_llm"`/`"sg_lang"`). `VLlm`/`VLlm` are **remote** OpenAI
  servers (a GPU box); the rest are local.
- **How/Where used:** Set at discovery (`llama_discover`, `vllm_discover`, llama.cpp
  tags, and the remote `/v1/models` query); matched in `run_prompt_inner`, eval, and
  compare dispatch.

#### File: `inference/backend/endpoint.rs`
- **Responsibility:** Resolve a backend to its base URL + optional bearer token,
  with deliberately non-colliding local ports.
- **Why:** The local sidecars may run at once.
  llama-server sits on **8081 not 8080** specifically so a stray `vllm_lm.server`
  (default 8080) can't shadow it — that exact collision made llama's `/health` pass
  while inference 404'd. The remote backends have no static default: their URL comes
  from `UserSettings` via `remote_config`.
- **What:** consts `LLAMA_SERVER` (8081), `vLLM_SERVER` (8082);
  `struct ResolvedEndpoint { url, api_key }`;
  `fn resolve(BackendKind) -> AppResult<ResolvedEndpoint>` (local = static/dynamic
  URL + no auth; vLLM reads its dynamic port; vLLM read `remote_config` and
  **error clearly when the URL is unset** — "set it in Settings", not an opaque
  connect error); `fn base_url(BackendKind) -> String` (url only, infallible — an
  unconfigured remote yields `""`, which probes treat as unavailable).
- **How/Where used:** `prompt.rs` resolves up front (so an unconfigured remote fails
  before the run token spins up); compare/eval `endpoint_for` helpers call `base_url`;
  health/discovery.

```rust
pub const LLAMA_SERVER: &str = "http://localhost:8081";
pub const LLAMA_SERVER: &str = "http://localhost:8081"; // NOT 8080
pub const vLLM_SERVER: &str = "http://localhost:8082";
pub struct ResolvedEndpoint { pub url: String, pub api_key: Option<String> }
pub fn resolve(kind: BackendKind) -> AppResult<ResolvedEndpoint> { /* local statics; remote from remote_config */ }
pub fn base_url(kind: BackendKind) -> String { resolve(kind).map(|r| r.url).unwrap_or_default() }
```

#### File: `inference/backend/remote_config.rs`
- **Responsibility:** Hold the user-configured remote endpoints (vLLM) as a
  process-global — the same pattern as `vllm/server/vllm_endpoint.rs`.
- **Why:** vLLM run on a remote GPU, so their URL + optional bearer key are
  user settings, and `inference/` can't read Tauri state. The settings command layer
  pushes them here on load and on every save; `endpoint::resolve` reads them.
- **What:** `struct RemoteEndpoint { url, api_key }`; `set_vllm`/`vllm` (setters trim blanks to `None` so an empty Settings field reads
  as "unconfigured").

---

## `inference/generate/` — the shared request/response shapes

#### File: `inference/generate/generate_spec.rs`
- **Responsibility:** The inputs to *one* generation, owned so the spec can move
  into a spawned task.
- **What:** `struct GenerateSpec { model, prompt, system: Option, options:
  Option<GenerateOptions>, keep_alive: Option<i32> }`.
- **How/Where used:** Built in `run_prompt_inner`; consumed by every
  `*_backend.rs`. Each backend uses the subset it needs (llama ignores `model`
  + `keep_alive`; vLLM uses `model` but not `keep_alive`; only llama.cpp uses
  `keep_alive`).

#### File: `inference/generate/generate_options.rs`
- **Responsibility:** The sampler knobs, named after llama.cpp's API.
- **Why:** One options struct shared by all three; each wire codec remaps field
  names (`num_predict` → llama `n_predict` → vLLM `max_tokens`).
- **What:** `struct GenerateOptions { temperature, top_p, top_k, num_predict,
  repeat_penalty, seed, num_ctx, stop }` (all `Option`, `skip_serializing_if`);
  `fn is_empty()` so an all-`None` options block is dropped before sending. `stop` is
  `Option<Vec<String>>` → llama.cpp `options.stop`; for models whose end-of-turn markers
  aren't a plain EOS (harmony `<|return|>`/`<|call|>`, gemma `<end_of_turn>`) these are
  what actually halt generation — the eval harness fills it per-model (see
  `backend-eval-engine.md` → `model_turn.rs`).
- **How/Where used:** Carried in `GenerateSpec.options`; `.filter(|o|
  !o.is_empty())` in each `stream_generate`.

#### File: `inference/generate/generate_stats.rs`
- **Responsibility:** Normalized final metrics, in milliseconds, every field
  optional.
- **Why:** Engines report different subsets — `None` means *"not measured"*,
  never a fabricated zero (`docs/architecture.md#robustness`).
- **What:** `struct GenerateStats { prompt_eval_count, prompt_eval_ms,
  eval_count, eval_ms, load_ms, total_ms }` and `fn ns_to_ms(u64) -> u64`
  (llama.cpp reports ns durations).
- **How/Where used:** Returned from every `generate`; the three `*_timings` /
  `*_stats` mappers produce it; UI renders TTFT/tok/s from this plus client-side
  `RunTiming`.

```rust
#[derive(Default, Clone, Serialize, PartialEq, Debug)]
pub struct GenerateStats {
    pub prompt_eval_count: Option<u32>,
    pub prompt_eval_ms: Option<u64>,
    pub eval_count: Option<u32>,
    pub eval_ms: Option<u64>,
    pub load_ms: Option<u64>,
    pub total_ms: Option<u64>,
}
pub fn ns_to_ms(ns: u64) -> u64 { ns / 1_000_000 }
```

---

## `inference/http/` — streaming transport primitives

#### File: `inference/http/http.rs`
- **Responsibility:** Construct the two reqwest clients and read error bodies safely.
- **Why:** Probes must fail fast; streams must not time out mid-transfer; error
  context must never be blanked.
- **What:** `probe_client()` (60s connect + 30s total, for HEAD/version/tags/blob
  checks), `streaming_client()` (60s connect, *no* body deadline — a multi-GB
  pull/long generation can run unbounded), `body_or_note(resp)` (annotates a
  failed body read rather than returning `""`). UA `quantamind/<version>` matters
  for HF endpoints behind Cloudflare.
- **How/Where used:** `streaming_client()` in all three `stream_generate`s and
  llama.cpp blob/create; `probe_client()` in blob existence checks.

#### File: `inference/http/ndjson.rs`
- **Responsibility:** Line framing for NDJSON/SSE byte streams.
- **What:** `next_line(&mut Vec<u8>) -> Option<Vec<u8>>` (pops one
  `\n`-terminated line, strips `\r\n`; `None` until a full line is buffered);
  `tail(&[u8]) -> Option<&[u8]>` (recovers a final un-terminated line, which some
  servers emit at end-of-stream).
- **How/Where used:** `next_line` drives the read loop in the llama.cpp and
  OpenAI-wire `stream_generate` paths.

```rust
pub fn next_line(buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    let nl = buf.iter().position(|&b| b == b'\n')?;
    let mut s: Vec<u8> = buf.drain(..=nl).collect();
    if s.last() == Some(&b'\n') { s.pop(); }
    if s.last() == Some(&b'\r') { s.pop(); }
    Some(s)
}
```

---

## `inference/token_handler.rs`

- **Responsibility:** Wrap the per-token emit closure with timing + cancellation.
- **Why:** Domain glue shared by the single-run and compare paths — not
  IPC-specific. An emit failure (channel closed) must *cancel the run*, not be
  swallowed.
- **What:** `make_token_handler(emit, cancel, timing) -> impl FnMut(&str)`. On
  `Ok(())` it records the token into the shared `RunTiming`; on `Err(())` it
  fires `cancel.cancel()`.
- **How/Where used:** `commands/prompt/prompt.rs` builds this and passes it as
  `on_token` into `run_prompt_inner`. This is where client-side TTFT / tokens-per-second
  come from (the engine stats only give counts/server-timings).

```rust
move |t| match emit(t) {
    Ok(())  => { timing.lock_recover().record_token(t); }
    Err(()) => { cancel.cancel(); }
}
```

---

## `inference/chat/` — chat-template detection (GGUF chat-template detection)

Used when importing a bare GGUF into llama.cpp: a `/completion`-style raw model
needs a chat template baked into its chat-template config. (llama-server now drives the
templated `/v1/chat/completions` with `--jinja`, so it uses the GGUF's *embedded*
template rather than these Go-template strings; only the legacy `/completion`
fallback prepends system text raw. vLLM applies its own template server-side. So


#### File: `inference/chat/chat_template_data.rs`
- **What:** `struct ChatTemplate { family, template_string, stop_tokens }` plus
  nine `const`s — `LLAMA3`, `QWEN_CHATML`, `MISTRAL`, `PHI3`, `GEMMA`,
  `COMMAND_R`, `DEEPSEEK`, `YI`, `GPT_OSS` — each a raw Go-template body (`{{ .System }}` /
  `{{ .Prompt }}` / `{{ .Response }}`) + its stop tokens. `GPT_OSS` (harmony) stops on
  `<|return|>` and `<|call|>` only — **not** `<|end|>`, which ends an intermediate message
  (stopping there would truncate the turn before the tool call).

#### File: `inference/chat/chat_templates.rs`
- **Responsibility:** Map a model to its template.
- **What:** `detect_template(model_name, architecture: Option<&str>) ->
  Option<ChatTemplate>` — prefers the GGUF architecture string
  (`by_architecture`), falls back to a name substring (`by_name`). `None` for
  unknown families so the caller can warn the user the install may produce
  broken output. Architectures `gpt-oss` → `GPT_OSS` and `gemma`/`gemma2`/`gemma4`
  → `GEMMA`. (The `gemma4` entry fixes the stop token only; it does NOT address the
  separate `gemma-4-12b-it-qat_q4_0` pad-token collapse, which is a broken-build issue.)

---

## The two backends

Each engine is one folder with the same five-part shape: `*.rs`
(`stream_generate` — HTTP + read loop), `*_backend.rs` (the trait impl),
`*_wire.rs` (request struct + chunk struct), and a stats/timings mapper.

### `inference/llama/`

The bundled `llama-server` sidecar; **single-model** (the GGUF is fixed at
spawn, so the request carries no model name).

#### File: `inference/llama/llama.rs`
- **Responsibility:** Stream from `llama-server`, **chat endpoint first**.
- **Why this ordering:** The PRIMARY path is the templated
  `/v1/chat/completions`. Combined with `--jinja` at spawn (see
  `commands/llama/llama_runtime.rs`), the server applies the GGUF's *embedded*
  chat template, so the model gets its trained turn structure, emits EOS, and
  stops. Posting a raw prompt to `/completion` (the old primary) applies **no**
  template — the model never emits EOS and loops to `n_predict`. That endpoint
  is now only a **404 fallback** for older builds; if it 404s too, the error
  names the likely port collision (e.g. `vllm_lm.server` on 8080). Both routes
  hit the same process — never a cross-`BackendKind` jump.
- **What:** `stream_generate(...)` orchestrates two helpers. `stream_chat` POSTs
  a llama-owned `ChatRequest` (`messages:[{system},{user}]`, keeps `seed` +
  `stop` — unlike vllm's request, which drops `seed`) and parses a llama-owned
  `ChatStreamChunk { choices, timings }`. It keeps the latest `timings` (llama's
  per-phase ms extension on the final chunk) → `Timings::stats()`, so
  `prompt_eval_ms` survives the chat endpoint (the Latency tab's TTFT breakdown
  needs it — token-count-only `usage` can't give it). `stream_completion` is the
  legacy raw path. Each returns `Ok(None)` on a 404 so the orchestrator falls
  through.

```rust
// chat endpoint is PRIMARY (templated via --jinja); /completion is the fallback.
if let Some(stats) = stream_chat(/* /v1/chat/completions */).await? { return Ok(stats); }
if let Some(stats) = stream_completion(/* legacy /completion */).await? { return Ok(stats); }
Err(AppError::Inference("neither /v1/chat/completions nor /completion is available …"))
// chat stream loop (shared OpenAI chunk): on_token(delta.content); stop on finish_reason.
```

#### File: `inference/llama/llama_backend.rs`
- **What:** `struct LlamaCppBackend { endpoint }`; `impl InferenceBackend`.
  `spec.model` + `spec.keep_alive` are *not* sent (single-model server).

#### File: `inference/llama/llama_wire.rs`
- **What:** `ChatRequest` (the primary path — `messages`, `stream:true`,
  `max_tokens`, and crucially `seed` + `stop` so llama.cpp runs stay
  seed-reproducible) + `ChatMessage`; `CompletionRequest` (legacy `/completion`:
  `prompt`, `n_predict` — not llama.cpp's `num_predict`, no `model`);
  `CompletionChunk { content, stop, timings }`; the chat-stream
  `ChatStreamChunk { choices, timings }` (+ `ChatChoice`/`ChatDelta`) — own type,
  not vllm's, because it must read llama's `timings` ms extension; `strip_sse(line)`
  removes a `data: ` prefix if present (bare JSON also OK).

#### File: `inference/llama/llama_timings.rs`
- **What:** `Timings { prompt_n, prompt_ms, predicted_n, predicted_ms }` (already
  ms, as f64). `stats()` rounds ms→u64 and fills four fields; **`load_ms` and
  `total_ms` stay `None`** — llama-server reports no load time.

```rust
GenerateStats {
    prompt_eval_count: self.prompt_n,
    prompt_eval_ms:    self.prompt_ms.map(|m| m.round() as u64),
    eval_count:        self.predicted_n,
    eval_ms:           self.predicted_ms.map(|m| m.round() as u64),
    load_ms: None, total_ms: None,
}
```

---

### `inference/openai/` — shared OpenAI-compatible SSE codec

The `/v1/chat/completions` streaming wire, shared by **every** backend that
speaks it: vLLM (llama.cpp reuses the chunk/stats
types on its own primary chat path). Each server is **multi-model** (the model id
*is* sent) and streams SSE. Extracted here (rather than living inside `inference/vllm/`)
so a new OpenAI-wire backend is one thin adapter over this codec — no cross-backend
`vllm::…` import.

#### File: `inference/openai/chat_stream.rs`
- **Responsibility:** Stream `/v1/chat/completions` (OpenAI SSE) for any such server.
- **What:** `stream_generate(endpoint, api_key: Option<&str>, model, prompt,
  system, options, think, cancel, on_token)`. When `api_key` is `Some`, attaches
  `Authorization: Bearer` (remote vLLM launched with `--api-key`); local
  vllm_lm.server passes `None`. The initial `.send()` is **raced against `cancel`**
  (`tokio::select! { biased; cancel … ; send … }`) because a wedged server (e.g. a
  non-chat model loaded) can accept the TCP connection but never return response
  headers, blocking `.send()` forever. Terminates on a choice's `finish_reason`, a
  `[DONE]` line, or cancel. A reasoning model's `delta.reasoning`/`reasoning_content`
  is re-wrapped as inline `<think>…</think>`.

```rust
let mut req = client.post(format!("{endpoint}/v1/chat/completions")).json(&body);
if let Some(key) = api_key.filter(|k| !k.is_empty()) { req = req.bearer_auth(key); }
let resp = tokio::select! {
    biased;
    _ = cancel.cancelled() => return Ok(GenerateStats::default()),
    r = req.send() => r.map_err(/* Timeout on connect, else Inference */)?,
};
// stream loop:
let payload = strip_sse(&line);
if payload == b"[DONE]" { return Ok(from_usage(usage)); }
if payload.first() != Some(&b'{') { continue; }      // skip SSE comments/framing
let chunk: ChatChunk = serde_json::from_slice(payload)?;
if chunk.usage.is_some() { usage = chunk.usage; }
if let Some(choice) = chunk.choices.into_iter().next() {
    if let Some(t) = choice.delta.content.filter(|t| !t.is_empty()) { on_token(&t); }
    if choice.finish_reason.is_some() { return Ok(from_usage(usage)); }
}
```

#### File: `inference/openai/chat_request.rs`
- **Gotcha (verified live):** the request sets `stream_options.include_usage:true`.
  vLLM **omit `usage` from streamed chunks** without it (token counts came
  back `None`), and they send that `usage` in a **separate trailing chunk** (choices
  `[]`) *after* the `finish_reason` chunk — so `chat_stream` records the finish reason
  but keeps reading until `[DONE]` rather than returning early. vllm_lm.server puts
  usage on the finish chunk and tolerates the flag, so it's unaffected.
- **What:** `ChatRequest` (OpenAI shape: `model`, `messages`, `stream:true`,
  `max_tokens` ← `num_predict`, `temperature`, `top_p`, `top_k`,
  `repetition_penalty` ← `repeat_penalty`, `chat_template_kwargs.enable_thinking`).
  System text becomes a `system` message. **No `seed`** — vllm_lm.server has no seed
  field, so these runs aren't seed-reproducible and the seed is intentionally dropped
  (vLLM accept the same body; unknown template kwargs are dropped by jinja).

#### File: `inference/openai/chat_chunk.rs`
- **What:** `ChatChunk { choices, usage }`, `Choice { delta, finish_reason }`,
  `Delta { content, reasoning }`, `Usage { prompt_tokens, completion_tokens,
  total_tokens }` (all optional — usage is version-dependent and may never arrive);
  `strip_sse(line)`. The reasoning field accepts both `reasoning` (vllm_lm.server)
  and `reasoning_content` (vLLM) via `#[serde(alias)]`.

#### File: `inference/openai/chat_stats.rs`
- **What:** `from_usage(Option<Usage>) -> GenerateStats` — maps token counts
  only; **every `*_ms` field stays `None`** (these servers report no per-phase
  timing). Absent usage → all-`None` default. TTFT/tok/s come from the client-side
  `RunTiming`, not here.

### `inference/vllm/`

`vllm_lm.server` on Apple Silicon; **multi-model**, OpenAI-compatible SSE — the wire
codec is the shared `inference/openai/` module above; this dir holds only the
adapter + Apple-Silicon gate + process management.

#### File: `inference/vllm/vllm_backend.rs`
- **What:** `struct VLlmBackend { endpoint, model }`; `impl InferenceBackend`.
  Delegates to `openai::chat_stream::stream_generate` with `api_key: None` (local,
  unauthenticated). Unlike llama, `spec.model` **is** sent; `keep_alive` has no vLLM
  equivalent.

#### File: `inference/vllm/mod.rs`
- **What:** `vllm_supported() -> bool` — the single `cfg!(all(macos, aarch64))`
  gate for the whole vLLM path (discovery/install/start are no-ops/errors
  elsewhere).

## Backend comparison

| | **llama.cpp** | **vLLM** |
|---|---|---|
| `BackendKind` | `LlamaCpp` (default) | `VLlm` / `VLlm` |
| Process | bundled `llama-server` sidecar | user-run on a remote GPU box |
| Port model | fixed `8081` | user-configured endpoint (`remote_config`) |
| Multi-model? | **no** (GGUF fixed at spawn) | whatever the server was launched with |
| Endpoint | `/v1/chat/completions` (templated via `--jinja`; → `/completion` fallback) | `/v1/chat/completions` |
| Wire format | OpenAI SSE (fallback: SSE-ish `data: {json}`) | OpenAI SSE |
| Request struct | `ChatRequest` (`max_tokens`, **keeps seed + stop**); fallback `CompletionRequest` (`n_predict`, no model) | `ChatRequest` (`max_tokens`, no seed) |
| Stop signal | `finish_reason` / `[DONE]` (fallback: `stop:true`) | `finish_reason` / `[DONE]` |
| Auth | none (loopback) | optional `Authorization: Bearer` |
| Stats source | `timings` object (ms) + `cache_n` | `usage` (counts only) |
| `GenerateStats` filled | 4 (`load_ms`/`total_ms` = None) | 2 counts only (all `*_ms` = None) |
| Health probe | `GET /health` | `GET /v1/models` |
| Lifecycle owner | `LlamaServerState` (one `Child`) | **not ours** — never spawned or reaped |
| Readiness | poll `/health` ≤30s (blocking start) | reachability probe + credential check |
| Reproducible seed? | yes | **no** (no seed field) |

### Remote backends — vLLM (`VLlm`) & vLLM (`VLlm`)

Both are **remote** OpenAI-compatible GPU servers (e.g. a GCP L4), so they differ
from the three local backends on exactly the axes that matter:

- **Not app-managed.** No spawn/reap/port/ownership guards, no `*ServerState`, no
  `app_lifecycle` entry — the app only points an HTTP client at a URL.
- **Endpoint + auth from Settings.** URL + optional `Authorization: Bearer` come from
  `UserSettings` via `remote_config`; `endpoint::resolve` errors clearly when unset.
- **Same wire as vLLM.** OpenAI SSE `/v1/chat/completions` via the shared
  `inference/openai/` codec (multi-model, `usage`-only stats, no seed). Native
  tool-calls via `openai::chat_tools` (bearer). Adapter: `inference/vllm/vllm_backend.rs`.
- **Health/discovery** via `commands/remote/` (`GET /v1/models` with bearer). The
  `model.backend` binding is **server-sourced** (`/v1/models`), so it never collides
  with vLLM's disk-sourced safetensors discovery.

This is a deliberate exception to the app's local-first posture (see the ADR under
`docs/adr/`).

---

## Server-process management (`commands/{llama,vllm,llama_cpp}/`)

All three share five robustness guards (see memory *spawned-process-robustness*):
log/health-gated readiness (not bare TCP-accept), reap on exit, dynamic/fixed
non-colliding ports, ownership handshake (only kill what we started), and a
stderr-aware launcher where loading is slow.

### `commands/llama/`

| File | Role |
|---|---|
| `llama_start.rs` | `start_llama_server` / `stop_llama_server` / `llama_server_info` (one-time spawn readout) commands. |
| `llama_runtime.rs` | spawn/probe/ready primitives + `check_llama_health`; `probe_running_model` (`/props` → `(model_path, n_ctx)`) surfaces an externally-started server the app didn't spawn. |
| `llama_server_types.rs` | `LlamaServerState` (one `Child`) + `LlamaStartResult`. |
| `llama_discover.rs` | scan dirs for `*.gguf` → `InstalledModelInfo{backend=LlamaCpp}`. |
| `llama_models.rs` | `list_llama_models` / `delete_llama_model` (symlink-safe). |
| `llama_templates.rs` | user/bundled `.jinja` override store; `resolve_template_file` (by model stem → arch), `list_chat_templates` IPC. |

- **`start_llama_server`** (`llama_start.rs`): takes `model_path` + optional
  `num_ctx` (the user's "Context window" param). If already reachable *and* serving
  this model **at the same `-c`** (`is_current`) → `AlreadyRunning`; a changed
  context relaunches (llama.cpp fixes context at spawn). Else `state.stop()` the
  previous, resolve the binary **directory** (`QUANTAMIND_LLAMA_DIR` → bundled
  `resources/binaries` → dev tree — the dir, not a lone binary, because
  `@loader_path` dylibs must stay colocated), spawn with
  `build_spawn_args(path, PORT, ctx, template)` where
  `ctx = resolve_launch_ctx(gguf_ctx, num_ctx, hw_ceiling)`: the user's `num_ctx` is
  honored (bounded by the GGUF max) so long prompts work, else the GGUF context
  **capped at `MAX_CONTEXT` 8K** (the declared value is the model MAX and `-c 262144`
  OOMs the KV cache) — and **either way** clamped by `hw_ceiling` (and floored at
  `MIN_CONTEXT` 2K). The ceiling is `hardware_ctx_ceiling(model_bytes, dims,
  total_memory)`: `USABLE_MEMORY_PCT` (70%) of **total** RAM minus the weights,
  divided by the per-token KV cost from `vram_math::calculate_kv_cache_bytes` over the
  GGUF's transformer dims (`spawn_meta` now reads `block_count` /
  `attention.head_count[_kv]` / `embedding_length` alongside `context_length`;
  per-block dims like gemma's array-typed `head_count_kv` reduce to their max via
  `as_dim_u64`, the GGUF reader keeping small int arrays). Total (not free) memory keeps
  the launched window a *stable* per-machine property; genuinely missing dims ⇒ **no RAM
  clamp** (`u32::MAX`) so an explicit window is never silently capped to a guess — the
  unset default still caps at 8K via `cap_context`. **Meter vs launch budget:** both go
  through `ceiling_from_per_token(usable, …)`, but the *launch* path passes
  `usable_memory_bytes(total, None)` (the 70% heuristic, unchanged), while the *meter* path
  `ctx_ceilings(model_bytes, dims, total, working_set)` passes the **measured** Metal
  working set (`GpuInfo::gpu_working_set_bytes`) on Apple Silicon — so the meters reflect
  what actually fits on the GPU. `ctx_ceilings` also returns a `FitVerdict`
  (`Fits`/`Tight`/`SpillsToCpu`/`Unknown` via `fit_verdict`) stating whether the weights
  fit under that limit at all — the question a large ceiling can't answer. `--jinja` always on; `template` is an
  optional `--chat-template-file` override resolved by `llama_templates` — `None`
  ⇒ the embedded template), then **block on
  `wait_until_ready()`** (poll `/health` every 500ms ≤30s). If readiness fails,
  kill and diagnose: a drained stderr tail naming a rejected `--jinja`
  (`jinja_unsupported`) → `JINJA_UNSUPPORTED_MSG` (stale bundled binary), else
  the generic timeout message. On the **ready** path it records a one-time
  `SpawnReadout { model_bytes, load_ms }`: `model_bytes` = the GGUF's on-disk size
  (`fs::metadata`, the dominant resident-memory term), `load_ms` = the spawn→ready
  wall-clock (model-load window; coarse, bounded by the 500ms poll). A
  failed/never-ready start records nothing (no fabricated number). `llama_server_info`
  exposes it — surfaced as a **spawn-time** readout in the Latency tab (NOT a
  per-request phase, since llama loads once at spawn and stays resident).
- **`spawn_server`** (`llama_runtime.rs`): sets `current_dir(dir)` +
  `DYLD_FALLBACK_LIBRARY_PATH=dir` so `@rpath` dylibs resolve; **pipes stderr**
  (drained on a thread into a bounded tail for the death diagnosis — an undrained
  pipe would wedge the child); kills by `Child` handle (portable, unlike llama.cpp's
  macOS `pkill`).
- **`LlamaServerState`**: one server per GGUF; a new model `stop()`s the prior;
  holds the `SpawnReadout` (set on ready, cleared on stop).

```rust
// llama_start.rs — HEALTH-gated readiness; on failure, diagnose stderr then reap
let mut child = spawn_server(&dir, &build_spawn_args(&model_path, PORT, ctx))?;
let tail = child.stderr.take().map(spawn_stderr_tail);
state.store(child, model_path);
if wait_until_ready().await { Ok(Started { pid, port: PORT }) }
else {
    let _ = state.stop();
    let stale = tail.map(|t| jinja_unsupported(&t.lock()...)).unwrap_or(false);
    Ok(StartFailed { error: if stale { JINJA_UNSUPPORTED_MSG } else { READY_TIMEOUT_MSG }.into() })
}
```

### `commands/vllm/`

| File | Role |
|---|---|
| `vllm_start.rs` | `start_vllm_server` / `stop_vllm_server` / `vllm_server_status`. |
| `vllm_server_types.rs` | `VLlmServerState`, `VLlmStartResult`, `VLlmServerStatus`, `Running`. |
| `health_vllm.rs` | `check_vllm_health` via `GET /v1/models`. |
| `vllm_discover.rs` | scan the weights folder for `*.gguf` → `LlamaCpp` models. |
| `vllm_install.rs` | `install_vllm_model` — HF snapshot download into `~/.quantamind/vllm`. |
| `vllm_models.rs` | `list_vllm_models` (from disk) / `delete_vllm_model` (symlink-safe). |

- **`start_vllm_server`** (`vllm_start.rs`): gate on `vllm_supported()`;
  `AlreadyRunning` if same model; `kill_all_servers()` otherwise; `locate` the
  exe; `find_available_port(8082)`; `spawn_server`; take stderr and
  `spawn_stderr_reader`; `state.store(Running{…}, port)` (which `set_vllm_port`).
  **Does NOT block on readiness** — weight load is slow; the UI polls
  `check_vllm_health` + `vllm_server_status` instead.
- **`VLlmServerState`** (`vllm_server_types.rs`): `store` sets the dynamic port;
  `kill_all_servers` clears it + reaps; `status()` uses `try_wait` to report
  `Running{phase,model}` vs `Exited{code, stderr_tail}` (the tail diagnoses the
  death). A `Drop` impl is the teardown backstop because `Child` *detaches* (does
  not kill) on drop — the primary reap is the `lib.rs` exit hook.

```rust
// vllm_start.rs — dynamic port + stderr-aware launcher, NON-blocking readiness
state.kill_all_servers()?;                          // ownership: stop a different model first
let exe = locate(configured.as_deref())?;           // → NotFound
let port = find_available_port(PORT_BASE)?;         // → NoFreePort
let mut child = spawn_server(&exe, &build_spawn_args(&model_path, port))?;
if let Some(err) = child.stderr.take() { spawn_stderr_reader(err, phase.clone(), tail.clone()); }
state.store(Running { child, model: model_path, phase, tail }, port);  // set_vllm_port(port)
Ok(VLlmStartResult::Started { pid, port })
```

### `commands/llama_cpp/`

| File | Role |
|---|---|
| `llama_cpp_start.rs` | `start_llama_cpp` / `stop_llama_cpp`; `llama.cppStartState`. |
| `llama_cpp_runtime.rs` | reachability probe, `resolve_llama_cpp`, spawn/kill, ready poll. |

- **Ownership handshake is the key guard here.** llama.cpp is the user's own daemon.
  `start_llama_cpp` only `remember`s a pid when *it* spawned `llama-server -m MODEL.gguf --port 8081 --jinja`; an
  `AlreadyRunning` result (a pre-existing user daemon) is **never** reaped.
  `stop_owned` kills only the app-spawned pid (used by `stop`, the exit reap, the
  signal reaper, and a `Drop` backstop). A separate `stop_llama_cpp` command uses
  `pkill -f "llama-server -m MODEL.gguf --port 8081 --jinja"` for an explicit user stop.
- **`kill_pid` is graceful-then-hard:** SIGTERM, a short grace (`kill -0` liveness
  poll, ~600ms), then SIGKILL if still alive — so the app-spawned llama.cpp can't
  outlive the app. Reached on **three** exit paths, all idempotent: Cmd+Q
  (`RunEvent::ExitRequested`), SIGINT/SIGTERM (the signal reaper), and **window
  close** (`on_window_event` → `reap_managed` + `app.exit(0)` — the macOS path,
  where closing the window doesn't otherwise quit the app and would orpha llama.cpp).
- `start_llama_cpp` guards a re-entrant `in_progress` flag; `start_llama_cpp_inner`
  short-circuits to `AlreadyRunning` if already reachable, else
  `resolve_llama_cpp()` (`which` → Homebrew/usr-local), `spawn_serve`, and **block
  on `wait_until_ready()`** (poll `/v1/models` ≤10s). Auto-start is macOS-only;
  elsewhere `spawn_serve`/`kill_serve` return `UNSUPPORTED_OS_MSG`.

```rust
// llama_cpp_start.rs — only reap a server WE spawned
if let llama.cppStartResult::Started { pid } = &result { state.remember(*pid); }
// stop_owned(): kill only the remembered pid; a user daemon (AlreadyRunning) is untouched
```

---

## Data-flow walkthrough — one streaming generation per backend

**llama.cpp** — `run_prompt(backend=llama.cpp)` → `prompt.rs` picks
`default_for(llama.cpp)` (`:8081`), wraps emit in `make_token_handler` →
`run_prompt_inner` → `llama.cppBackend::generate` → `llama_cpp::stream_generate`:
`streaming_client()` POSTs `GenerateRequest{stream:true, …}` to `/v1/chat/completions`;
the read loop splits NDJSON lines, deserializes `GenerateChunk`, calls
`on_token(chunk.response)`; on `done:true` returns `chunk.stats()` with all six
ms/count fields (ns→ms). Cancel mid-stream returns the all-`None` default.

**llama.cpp** — endpoint `:8081`. `LlamaCppBackend::generate` →
`llama::stream_generate`: PRIMARY POST `ChatRequest{messages:[{system},{user}],
seed, stop, stream:true}` to `/v1/chat/completions` — with `--jinja` at spawn the
server applies the GGUF's embedded template, so the model emits EOS and stops
(a raw `/completion` prompt has no template and loops). Stream via
`ChatStreamChunk`: `on_token(delta.content)`; stop on `finish_reason`; the final
chunk's `timings` → `Timings::stats()` so `prompt_eval_ms` (the Latency tab TTFT
breakdown's prefill) survives. If the chat route 404s (older build) → fall back to
legacy `/completion` (`CompletionChunk`, `stop:true`); if *that* 404s too →
port-collision error.

**vLLM** — endpoint = `vllm_endpoint()` (the managed dynamic port).
`VLlmBackend::generate` → `vllm::stream_generate`: the `.send()` of
`ChatRequest{model, messages, stream:true}` is **raced against cancel** (a
wedged model can stall headers). Then `next_line` → `strip_sse` → skip non-`{`
SSE framing/`[DONE]` → `ChatChunk`; emit `choice.delta.content`; capture `usage`
if present; on `finish_reason` or `[DONE]` return `from_usage(usage)` (token
counts only, all `*_ms` = None). Throughout, `make_token_handler` records each
token into `RunTiming` so the UI still gets TTFT + tokens/sec the vLLM server
never reports.

---

## Design invariants (don't break these)

1. **Backend = weight format, decided once at discovery.** Dispatch matches
   `BackendKind` and nothing else; never fall back across engines on a health
   signal.
2. **Never fabricate metrics.** A missing stat is `None` ("Not available"), not
   `0`. Client-side timing (`RunTiming`) is the only source of TTFT/tok-s.
3. **Cancellation is honored everywhere** — before/around `.send()` (vLLM),
   inside the read loop, and on emit failure (`make_token_handler`).
4. **Readiness ≠ TCP-accept.** Health is `/health`- and `/v1/models`-gated;
   stderr is advisory phase only.
5. **Only reap what we started.** `LlamaServerState` owns its one `Child` and reaps
   on stop + Drop + exit hook; a remote server is never ours to touch.
6. **The sidecar port is deliberately 8081, not 8080**, so a hand-launched
   OpenAI-compatible server on the community default can't shadow it.
