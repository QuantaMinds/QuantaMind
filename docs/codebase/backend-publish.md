# Backend — Publish Subsystem

The **publish** subsystem lets a user share a benchmark/readiness result from their
local machine to a public community leaderboard ("the board") hosted at
`https://api.quantamind.co`. It owns OAuth/PKCE identity, the canonical wire payload,
a "what's shared" preview, local pre-validation, and the actual HTTP send.

> **Two repos.** Publish spans the **desktop app** (this repo — the client documented
> here) and the **website/server** repo (the closed backend that owns `/authorize`,
> `/token`, `/publish/nonce`, `/publish`, dedup, and authoritative validation). This
> doc covers the desktop side only.

- Readiness data being published comes from **[backend-eval-engine.md](./backend-eval-engine.md)** (`ModelVerdict`).
- Canonical / row / validate storage details are also summarized in **[backend-persistence.md](./backend-persistence.md)**; the publish-specific semantics live here.
- The Publish UI (`PublishButton` / `PublishDialog` under `features/publish`, used by the Agent Report) is **[frontend-inspector-quant-agentreport.md](./frontend-inspector-quant-agentreport.md)**.

---

## Overview

### Why publish exists
A user runs an agentic eval locally and gets a per-model readiness verdict. Publish
turns the *aggregate, metrics-only* slice of that verdict into a leaderboard row so a
community can compare which models pass on which hardware. No prompt, no task content,
no completion text ever leaves the machine — only ranked aggregates.

### What is shared — the canonical row
One `PublishRow` is sent **per measured tool-calling path**: a model evaluated on both
native and prompt-based publishes **two** rows, distinguished by `eval_method`
(`native_fc` / `prompt_based`) — `build_preview` projects each `ModelVerdict`, and the
Agent Report now emits one verdict per path. ⚠ The hosted server's dedup key must therefore
be `UNIQUE(user, model, quant, cohort_key, eval_method)` (not `…, cohort_key`) or the
second path 422s and the publish silently half-succeeds — this server change is **coupled**
and must land with/before the client. `pre_validate` does **not** dedup by model, so both
rows pass locally (pinned by a test).

The wire shape is `PublishRow` (`persistence/publish/row.rs`): `model`, `quant`,
`cohort_key` (derived hardware bucket), `tool_version`, a metrics bag (`pass_k`
required; `effort`/`avg_steps` optional), and — since the Phase 9 extension — the
graduated tier verdict (`status`, `eval_method`, `tier_tested`, `cleared_tier`,
`hardware_class`, `recommended_tier`), the per-tier saturation curve
(`by_tier: [{tier, pass_k_rate, k, avg_steps?, decoy_count?}]`), the failure
**distribution** (`failure_distribution` — counts by mode, never the failing runs),
the **reasoning-budget context** (schema v2: `is_thinking`, `think_preset` = the
Lean/Standard/Deep thinking budget, `think_budget?` = the scratchpad token cap at the
tier tested — `None` for a terse model, never a fabricated 0, `ctx_ceiling?`, and
`cpu_offloaded`), so the board can read a reasoning model's token `effort`/reliability
in context, the collection identity (`collection_name` + `collection_hash`), and build
provenance (`schema_version`, `engine_version`, `build_hash`). The row is built by **allowlist**:
everything in `ModelVerdict` not named here (verdict reasons, memory profile, backend
internals, traces) is dropped, so a new `ModelVerdict` field is private until someone
adds it to `project` on purpose. Rows are serialized to **canonical JSON** (sorted
keys, no whitespace) and a **SHA-256 hash** is sent alongside for transit-integrity —
the hash covers the full extended row deterministically.

### How — IPC commands
| Command | File | Gate | Purpose |
|---|---|---|---|
| `save_readiness_image(path, bytes)` | `export_cmd.rs` | always built | Offline: write the readiness card PNG to disk. No auth, no network. |
| `start_login(app, state)` | `identity/login_cmd.rs` | `not(enterprise)` | PKCE browser sign-in → caches access token, stores rotated refresh token. |
| `preview_publish_payload(verdicts, params, collection_id, collection_hash)` | `preview_cmd.rs` | `not(enterprise)` | Build the exact payload (rows + canonical JSON + hash + cohort + excluded count + first validation error). `collection_id` stamps the display name; `collection_hash` is the RUN-VERIFIED hash (from the report) that gates publishability — `None` for custom/edited collections. Offline. |
| `publish_to_board(state, verdicts, params, collection_id, link)` | `publish_cmd.rs` | `not(enterprise)` | Validate → resolve token → POST one batch to `api.quantamind.co`. |

### Managed state & the enterprise gate
- **`AuthState`** (`auth_state.rs`) is `.manage()`d in `lib.rs` in **every** build (un-gated)
  so the type always exists; it caches only the short-lived **access** token in memory.
- The **auth + preview + send** surface (`identity/`, `cohort`, `preview_cmd`,
  `publish_cmd`) compiles **out** of `enterprise`/air-gapped builds via
  `#[cfg(not(feature = "enterprise"))]` at `mod.rs` and at the `tauri::command` handler
  list in `lib.rs`. The offline `export_cmd` stays **in** every build.

---

## `commands/publish/` — command layer (IPC + network + identity)

### File: `mod.rs`
- **Responsibility:** Declare the publish command modules and apply the enterprise gate.
- **Why:** Air-gapped/enterprise builds must not ship the auth/send surface; only the
  offline PNG export survives.
- **What:** `auth_state` + `export_cmd` always public; `identity`, `cohort`,
  `preview_cmd`, `publish_cmd` behind `not(feature = "enterprise")`.
- **How/Where used:** Compiled into the `commands` tree; handlers wired in `lib.rs`.

```rust
pub mod auth_state;
pub mod export_cmd;
#[cfg(not(feature = "enterprise"))] pub mod identity;
#[cfg(not(feature = "enterprise"))] pub mod cohort;
#[cfg(not(feature = "enterprise"))] pub mod preview_cmd;
#[cfg(not(feature = "enterprise"))] pub mod publish_cmd;
```

### File: `auth_state.rs`
- **Responsibility:** Hold the cached short-lived **access** token for the session.
- **Why:** Avoid re-refreshing on every publish; un-gated so `lib.rs` can `.manage()` it
  in every build (an empty cache in enterprise is harmless).
- **What:** `AuthState { access: Mutex<Option<String>> }` with `cached()`, `set()`,
  `clear()`. The **refresh** token never lives here — it goes to the OS vault (`auth.rs`).
- **How/Where used:** `.manage()`d in `lib.rs`; read by `access_token()`; `clear()`ed on a 401.

```rust
#[derive(Default)]
pub struct AuthState { access: Mutex<Option<String>> }
impl AuthState {
    pub fn cached(&self) -> Option<String> { self.access.lock_recover().clone() }
    pub fn set(&self, token: String) { *self.access.lock_recover() = Some(token); }
    pub fn clear(&self) { *self.access.lock_recover() = None; } // after a 401, force refresh
}
```

### File: `cohort.rs`
- **Responsibility:** Derive a deterministic **hardware cohort key** so verdicts on
  comparable machines pool together.
- **Why:** The leaderboard ranks within a cohort; the same hardware must always produce
  the same token across machines, and the **server's bucketing must match exactly** or the
  `UNIQUE(user, model, quant, cohort_key)` dedup breaks.
- **What:** `cohort_key(hw) -> "{platform}/{accel}/{mem_tier}"`. Platform/accel split by
  `is_apple_silicon` (→ `apple_class` e.g. `m3-pro`), else a usable GPU (`gpu_vendor` +
  `slug`), else CPU arch. `mem_tier` buckets RAM (`0-8gb` … `128gb+`). **Quant is NOT in
  the cohort** — it is a separate dedup column.
- **How/Where used:** `preview_cmd::preview_publish_payload` and
  `publish_cmd::publish_to_board` call it on the **authoritative LOCAL** `snapshot()` —
  never a frontend-supplied snapshot.

```rust
pub fn cohort_key(hw: &HardwareSnapshot) -> String {
    let (platform, accel) = if hw.is_apple_silicon {
        ("apple-silicon".to_string(), apple_class(&hw.cpu))
    } else if let Some(name) = hw.gpu.name.as_deref().filter(|_| hw.gpu.available) {
        (gpu_vendor(name).to_string(), slug(name))
    } else { ("cpu".to_string(), slug(&hw.arch)) };
    let accel = if accel.is_empty() { "unknown".to_string() } else { accel };
    format!("{platform}/{accel}/{}", mem_tier(hw.total_memory_bytes))
}
```

### File: `export_cmd.rs`
- **Responsibility:** Write the readiness card PNG to disk (the share-by-image path).
- **Why:** Offline sharing must work in every build, including enterprise — no auth, no
  network.
- **What:** `save_readiness_image(path, bytes)` → thin sink: validate non-empty path &
  bytes, `fs::write`. The frontend snapshots the card to raw bytes and picks the path via
  the OS save dialog (Tauri file-I/O-belongs-in-Rust pattern: React passes only the path).
- **How/Where used:** Always-built IPC command; the PNG round-trip is test-verified by the
  embedded `tests` module.

```rust
#[tauri::command]
pub fn save_readiness_image(path: String, bytes: Vec<u8>) -> Result<(), AppError> {
    save_inner(&path, &bytes) // empty-path / empty-bytes rejected, then fs::write
}
```

### File: `preview_cmd.rs`
- **Responsibility:** Build the **"what's shared" preview** — exactly the bytes that would
  be sent, with no network.
- **Why:** The privacy-gate dialog must show the user the projected rows, the canonical
  JSON + hash, the derived cohort, how many models were dropped as unmeasured, and any
  local validation failure — *before* anything leaves the machine.
- **What:** `PublishPreview { rows, canonical_json, hash, cohort_key, excluded_count,
  invalid: Option<InvalidRow> }`. `publish_context` assembles the run-wide
  `PublishContext` (cohort + hardware class from the local snapshot; collection
  identity/hash + per-tier decoy axes from the active `collection_id`; engine/build
  provenance); `build_preview` is the pure core (context injected) so the previewed
  payload is **byte-identical** to the sent one and is unit testable.
  Unmeasured/unquantized/custom-collection verdicts are dropped (`excluded_count`).
- **How/Where used:** `preview_publish_payload(verdicts, params, collection_id)` IPC handler
  builds the context then calls `build_preview`. Shared with `publish_cmd`.

```rust
pub(crate) fn build_preview(verdicts: &[ModelVerdict], ctx: &PublishContext)
    -> Result<PublishPreview, AppError> {
    let rows: Vec<PublishRow> = verdicts.iter().filter_map(|v| PublishRow::project(v, ctx)).collect();
    let excluded_count = verdicts.len() - rows.len();
    let invalid = pre_validate(&rows).err().map(|(index, reason)| InvalidRow { index, reason });
    Ok(PublishPreview { canonical_json: canonical_json(&rows)?, hash: canonical_hash(&rows)?,
        cohort_key: ctx.cohort_key.clone(), excluded_count, invalid, rows })
}
```

### File: `publish_cmd.rs`
- **Responsibility:** Resolve the API host, build the batch, send it, and map every server
  status to a UI-actionable outcome.
- **Why:** A failed publish must never throw an opaque error that freezes the dialog —
  every status becomes an explicit next step.
- **What:**
  - `publish_api()` resolves the base once: `QM_API_BASE` env override (e.g. local
    `http://localhost:8787`) else **`https://api.quantamind.co`**.
  - `PublishOutcome`: `Ok { board_url }`, `NeedsAuth`, `Invalid { index }`,
    `UpdateRequired` (426), `RateLimited` (429).
  - `publish_batch`: GET a fresh **nonce** (server burns it on a 422), then POST
    `{ nonce, hash, results, link? }` with `bearer_auth(token)`. Status → outcome.
  - `publish_to_board` (IPC): rebuilds the previewed payload, short-circuits on local
    `invalid`, resolves an access token (→ `NeedsAuth` if none), sends one batch, and
    **clears the cached token** if the server returns `NeedsAuth` so the next try re-auths.
- **How/Where used:** IPC handler invoked by the Publish dialog after the user confirms.

```rust
pub fn publish_api() -> &'static str {
    static API: OnceLock<String> = OnceLock::new();
    API.get_or_init(|| std::env::var("QM_API_BASE")
        .unwrap_or_else(|_| "https://api.quantamind.co".to_string())).as_str()
}
// one batch = one fresh nonce + one POST; status → PublishOutcome
let body = PublishRequest { nonce, hash, results: rows, link };
let resp = client.post(format!("{base}/publish")).bearer_auth(token).json(&body).send().await?;
```

---

## `commands/publish/identity/` — OAuth/PKCE + token vault

### File: `mod.rs`
- **Responsibility:** Group the identity concern (PKCE, sign-in, token endpoints, vault)
  in its own subfolder to stay under the folder-taxonomy limit.
- **Why:** The whole subfolder is gated at the parent (`not(enterprise)`), so no per-module
  `cfg` is needed; the un-gated `AuthState` lives one level up.
- **What/Where used:** `pub mod auth; login_cmd; pkce; token;`.

### File: `pkce.rs`
- **Responsibility:** PKCE primitives + the loopback OAuth redirect catcher.
- **Why:** Public-client OAuth without a client secret needs a PKCE
  challenge/verifier pair, and a desktop app catches the auth code on a loopback port.
- **What:**
  - `pkce_challenge(verifier)` = `base64url(no-pad)` of `SHA-256(verifier)` (the S256
    method) — pure & deterministic so client and server agree byte-for-byte.
  - `pkce_pair()` = a fresh verifier (two v4 UUIDs in hex, 64 chars — within PKCE's
    43–128 range, no new RNG dep) + its challenge.
  - `parse_code_from_request(raw)` pulls `code=` out of the raw HTTP request line.
  - `await_redirect(listener)` blocks on the **single** loopback redirect, returns the
    `code`, and serves a "you can close this tab" page.
- **How/Where used:** `login_cmd::start_login` mints the pair, binds the listener, and
  awaits the redirect.

```rust
pub fn pkce_challenge(verifier: &str) -> String {        // S256
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}
pub fn pkce_pair() -> (String, String) {                 // 64-char verifier, no new RNG dep
    let verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let challenge = pkce_challenge(&verifier);
    (verifier, challenge)
}
```

### File: `login_cmd.rs`
- **Responsibility:** Orchestrate the full PKCE browser sign-in.
- **Why:** Thin, tested glue over the pkce/redirect/exchange pieces, with a pre-flight
  reachability probe and a timeout so a down or never-completed sign-in can't hang.
- **What:** `start_login(app, state)`:
  1. `ensure_reachable(publish_api())` — GET `/authorize` with a **5s** timeout; ANY HTTP
     response = reachable, a transport error = "Can't reach the publish server…".
  2. Mint `pkce_pair()`; bind an **ephemeral** `127.0.0.1:0` listener; build
     `redirect = http://127.0.0.1:{port}/callback`.
  3. Open the system browser to `/authorize?response_type=code&scope=publish&
     code_challenge_method=S256&code_challenge=…&redirect_uri=…`.
  4. `await_redirect` with a **300s** timeout → the auth `code`.
  5. `exchange_code` → tokens; `store_refresh_token`; cache the access token in `AuthState`.
  6. Return `true` iff the refresh token reached durable **keychain** storage (`false` =
     session-only, so the UI can warn "may need to sign in again next launch").
- **How/Where used:** IPC command triggered when publish needs auth.

```rust
let url = reqwest::Url::parse_with_params(&format!("{}/authorize", publish_api()),
    &[("response_type","code"), ("scope","publish"),
      ("code_challenge_method","S256"), ("code_challenge",&challenge),
      ("redirect_uri",&redirect)])?;
app.shell().open(url.to_string(), None)?;
let code = timeout(LOGIN_TIMEOUT, await_redirect(listener)).await??; // 300s
let tokens = exchange_code(publish_api(), &code, &verifier).await?;
let persisted = store_refresh_token(&tokens.refresh_token);
state.set(tokens.access_token);
Ok(persisted == Persisted::Keychain)
```

### File: `token.rs`
- **Responsibility:** OAuth token endpoints + resolve a usable access token.
- **Why:** Keep network token logic pure (no keychain touch) so it is mockito-testable;
  storage is the caller's job.
- **What:**
  - `Tokens { access_token, refresh_token, expires_in }`. The refresh token **rotates** on
    every call (server revokes the old one) — always re-store what comes back.
  - `exchange_code(base, code, verifier)` → POST `/token`
    (`grant_type=authorization_code` + `code` + `code_verifier`).
  - `refresh_access(base, refresh)` → POST `/token/refresh`.
  - `access_token(base, state)` → cached token, else refresh from vault (rotate + re-store),
    else `NeedsAuth`. Never panics.
- **How/Where used:** `login_cmd` (`exchange_code`); `publish_cmd::publish_to_board`
  (`access_token`).

```rust
pub async fn access_token(base: &str, state: &AuthState) -> Result<String, NeedsAuth> {
    if let Some(t) = state.cached() { return Ok(t); }
    let refresh = get_refresh_token().ok_or(NeedsAuth)?;
    let tokens = refresh_access(base, &refresh).await.map_err(|_| NeedsAuth)?;
    store_refresh_token(&tokens.refresh_token); // rotated
    state.set(tokens.access_token.clone());
    Ok(tokens.access_token)
}
```

### File: `auth.rs`
- **Responsibility:** The OS-keychain refresh-token vault with a session fallback.
- **Why:** The refresh token is the long-lived credential; it must survive restarts when
  possible but never strand the user if the keychain is locked/denied/absent.
- **What:** Keyring service `"quantamind"`, user `"publish-refresh"`.
  - `store_refresh_token` — **always** writes the in-memory session copy first, *then*
    best-effort to the OS store; returns `Persisted::Keychain` or `Persisted::SessionOnly`.
  - `get_refresh_token` — prefer the session copy (no re-prompt this launch); on a cold
    session read the store and cache a hit.
  - `clear_refresh_token` — best-effort wipe of both stores (logout/revoke).
- **How/Where used:** `login_cmd`, `token.rs`. `Persisted` propagates to `start_login`'s
  boolean return so the UI surfaces the session-only / denied-keychain state (never silent).

```rust
pub fn store_refresh_token(token: &str) -> Persisted {
    *mem().lock_recover() = Some(token.to_string()); // session copy first — can't be stranded
    match keyring::Entry::new(SERVICE, REFRESH_USER).and_then(|e| e.set_password(token)) {
        Ok(()) => Persisted::Keychain,
        Err(_) => Persisted::SessionOnly,
    }
}
```

---

## `persistence/publish/` — canonical wire record (pure leaf, Tauri-free)

This module builds the exact deterministic wire structure the closed server validates.
`cohort_key` derivation lives a layer up (it needs `HardwareSnapshot`); this stays a
dependency leaf. Also referenced from [backend-persistence.md](./backend-persistence.md);
publish-specific semantics are below.

### File: `mod.rs`
- **Responsibility / What:** Declare `canonical`, `row`, `validate`.
- **Why:** Group the pure publish record shape away from Tauri/command code.

### File: `row.rs` — the canonical row + publishability rule
- **Responsibility:** Define `PublishMetrics`, `TierMetric`, `FailureDistribution`,
  `PublishRow` + `PUBLISH_SCHEMA_VERSION`, and the `PublishContext` that `project`
  threads, then project a `ModelVerdict` into a publishable row by **allowlist**.
- **Why:** This is the **whole wire shape**; everything not named here is dropped, and the
  projection is the **client half of the null-poisoning guard** plus the
  custom-collection exclusion.
- **What:**
  - `PublishMetrics { pass_k: f64 (required), effort?, avg_steps? }`. Soft metrics omitted
    when unmeasured (`skip_serializing_if = "Option::is_none"`) — the JSONB bag stays
    additive/forward-compatible. **No task content ever lives here.**
  - `TierMetric { tier, pass_k_rate, k, avg_steps?, decoy_count? }` — one tier's point on
    the saturation curve (`k` from `pass_k_for`, `decoy_count` from the collection axes).
  - `FailureDistribution { infinite_loop, hallucinated, malformed_json, schema_unrecovered,
    unknown_tool_calls, forbidden_calls, turn_timeouts, reported_in_prose }` — counts only,
    mapped **field-by-field** from `FailureTracker` (NOT serialized directly, so a new
    tracker counter never auto-publishes).
  - `PublishRow { model, quant, cohort_key, tool_version, metrics, params, status,
    eval_method, tier_tested?, cleared_tier?, hardware_class, recommended_tier, by_tier,
    failure_distribution, is_thinking, think_preset, think_budget?, ctx_ceiling?,
    cpu_offloaded, collection_name, collection_hash, schema_version, engine_version,
    build_hash }`. Run-wide fields are repeated per row (matching the `cohort_key`/
    `tool_version` precedent) so the canonical hash stays one hash over `[PublishRow]`.
    `think_budget` is computed in `project` = `think_tokens_for_preset(tier_tested, preset)`
    when `is_thinking`, else `None`. `PUBLISH_SCHEMA_VERSION = 2` (reasoning-budget extension).
  - `project(v, ctx) -> Option<PublishRow>` returns **`None` unless the verdict has a
    measured `pass_k`, a real `quantization`, AND `ctx.collection_hash` is `Some`** — i.e.
    **a row needs pass_k + quantization + a built-in collection to be publishable**. A
    `None` collection hash (custom/user-authored collection) excludes the row, alongside
    the unmeasured/unquantized exclusions that would skew the server's baseline `n`.
- **How/Where used:** `build_preview` filters verdicts through `project`; the dropped count
  becomes `excluded_count`. `PublishContext` is assembled by `preview_cmd::publish_context`.
- **Fork-on-edit (Slice 4) — `collection_hash` is RUN-VERIFIED, the single source of truth.**
  `ctx.collection_hash` is NO LONGER re-derived from `collection_id` at publish time (that let an
  *edited* bundled collection publish under the real hash). It is computed once at run time
  (`batch_cmd::verified_collection_hash`): `Some(collection_hash(id))` ONLY when the run's received
  tasks byte-for-byte equal the pristine `load_v2_collection` (serde `Value`, exact — zero
  tolerance); `None` for a custom id OR ANY edit. It rides on `BatchReport.collection_hash` and is
  threaded into `preview_publish_payload`/`publish_to_board` → `publish_context`. So a doctored or
  user-edited collection is unpublishable by construction; editing the in-app snapshot
  (`world_state`) forks it automatically. Pinned by near-miss (one-char edit → `None`) +
  round-trip-completeness tests.

```rust
pub fn project(v: &ModelVerdict, ctx: &PublishContext) -> Option<PublishRow> {
    let pass_k = v.pass_k?;                          // must be measured
    let quant = v.quantization.clone()?;             // must be a real quantization
    let collection_hash = ctx.collection_hash.clone()?; // must be a built-in collection
    // … map by_tier (rate/k/decoy), tier_tested = max tier, failure_distribution …
}
```

### File: `canonical.rs` — deterministic JSON + integrity hash
- **Responsibility:** Serialize the batch to canonical JSON and hash it.
- **Why:** Two clients (or client and server) that build the same logical batch must produce
  **byte-identical** JSON; an unordered map would change the hash and read as tampered.
- **What:** `canonicalize` recursively sorts every object's keys (via `BTreeMap`, so order
  holds regardless of serde's `preserve_order`). `canonical_json` = sorted-key, no-whitespace
  string. `canonical_hash` = lowercase-hex SHA-256 over that string — the integrity hash sent
  with the batch (TLS + bearer token + nonce + this hash close transit tampering;
  self-fabrication is a separate server concern).
- **How/Where used:** `build_preview` fills `canonical_json` + `hash`; `publish_batch` sends
  the hash in the request body.

```rust
pub fn canonical_hash(rows: &[PublishRow]) -> AppResult<String> {
    let mut hasher = Sha256::new();
    hasher.update(canonical_json(rows)?.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}
```

### File: `validate.rs` — local pre-validation
- **Responsibility:** Re-run the server's plausibility checks locally so a malformed row
  never enters the batch.
- **Why:** The server stays a backstop, not the primary gate; the user sees the failing row
  in the preview dialog before sending. Authoritative validation remains server-side.
- **What:** `pre_validate(rows) -> Result<(), (usize, String)>` returns the index of the
  **first** offending row + a reason naming the field & value (mirrors the server's
  422-with-index). Checks: non-empty `model`/`quant`/`cohort_key`; `pass_k` in `0.0..=1.0`
  and not NaN; `effort > 0`; `avg_steps >= 0`.
- **How/Where used:** `build_preview` runs it → `PublishPreview.invalid`;
  `publish_to_board` short-circuits to `Invalid { index }` before any network.

```rust
if !(0.0..=1.0).contains(&m.pass_k) || m.pass_k.is_nan() {
    return bad(format!("pass_k {} out of range 0..=1", m.pass_k));
}
```

---

## Publish flow — steps, command, guard

| Step | Driver (command) | Guard at this step |
|---|---|---|
| **1. Login** | `start_login` | Pre-flight reachability probe (5s) to `/authorize`; PKCE S256 challenge; ephemeral loopback redirect; 300s sign-in timeout; refresh token rotated + vaulted (keychain or session-only). |
| **2. Token resolve** | `access_token` (inside `publish_to_board`) | Cached access token, else refresh-from-vault (rotate + re-store), else `NeedsAuth` → UI triggers login. |
| **3. Preview** | `preview_publish_payload` | Offline-only; cohort from **local** `snapshot()`; unmeasured/unquantized rows dropped (`excluded_count`); shows exact canonical JSON + hash. |
| **4. Validate** | `pre_validate` (inside preview/publish) | Row needs **pass_k + quantization** to exist at all (`project`); then range/empty checks → `Invalid { index }`. |
| **5. Publish** | `publish_to_board` → `publish_batch` | Fresh nonce per attempt; bearer token; integrity hash; status → `Ok{board_url}` / `NeedsAuth` (clears cache) / `Invalid` / `UpdateRequired` (426) / `RateLimited` (429). |
| **(any time) Export** | `save_readiness_image` | Offline PNG; non-empty path & bytes; ships in every build incl. enterprise. |

---

## Security & privacy

- **What leaves the machine:** only the projected `PublishRow`s — `model`, `quant`,
  `cohort_key`, `tool_version`, and the metrics bag (`pass_k` + optional `effort`/`avg_steps`)
  — plus the canonical-JSON SHA-256 hash, a server nonce, and an optional user-supplied
  `link`. Over TLS with a bearer access token.
- **What does NOT leave:** prompts, task/dataset content, completion text, verdict reasons,
  memory profiles, backend internals, the raw hardware string, and — critically — the
  **refresh token** (kept in the OS keychain; only the short-lived access token is cached
  in process memory).
- **Loopback OAuth redirect:** the auth code is caught on an **ephemeral 127.0.0.1** port
  (`bind("127.0.0.1:0")`); the `redirect_uri` is built from the OS-assigned port, so no fixed
  port and nothing is exposed off-host. Exactly one redirect is served, then a static
  "close this tab" page.
- **PKCE:** S256 challenge/verifier means no client secret ships in the desktop binary; the
  verifier is sent only at `/token` exchange. Refresh tokens **rotate** on every use.
- **"What's shared" preview:** `preview_publish_payload` is a hard privacy gate — the user
  sees the *byte-identical* payload (`build_preview` is shared with the send path) before any
  network call. `excluded_count` tells them how many models were dropped as unmeasured.
- **Enterprise gate:** air-gapped builds compile the entire auth/send/cohort/preview surface
  out; only offline PNG export remains.

---

## Data-flow walkthrough

```
User clicks Publish (Agent Report → PublishButton/PublishDialog)
   │
   ├─(if NeedsAuth) start_login ──────────────────────────────────────────────┐
   │     ensure_reachable(/authorize, 5s) → pkce_pair() (S256)                 │
   │     bind 127.0.0.1:0 → open browser /authorize?...&code_challenge=...     │
   │     await_redirect (300s) → code → exchange_code(/token, verifier)        │
   │     store_refresh_token (keychain|session) ; AuthState.set(access)        │
   │                                                                            │
   ▼                                                                            │
preview_publish_payload(verdicts)  ◄── ModelVerdict from eval engine           │
   cohort_key(snapshot())  ;  PublishRow::project (needs pass_k + quantization) │
   pre_validate → invalid?  ;  canonical_json + SHA-256 hash  → shown to user   │
   │                                                                            │
   ▼  (user confirms)                                                          │
publish_to_board(state, verdicts, link)                                        │
   build_preview again (byte-identical) → invalid? → Invalid{index}            │
   access_token(api, state) ──(none)── NeedsAuth ─────────────────────────────┘
   │  (token)
   ▼
publish_batch:  GET api.quantamind.co/publish/nonce  (bearer)
                POST api.quantamind.co/publish  { nonce, hash, results, link? }
   │
   ▼
PublishOutcome → Ok{board_url} | NeedsAuth(clear cache) | Invalid{index}
                 | UpdateRequired | RateLimited   →  UI renders next step
```

The published readiness numbers originate in the eval engine
([backend-eval-engine.md](./backend-eval-engine.md)); the wire record shape is shared with
[backend-persistence.md](./backend-persistence.md); the dialog that drives this flow is
documented in [frontend-inspector-quant-agentreport.md](./frontend-inspector-quant-agentreport.md).
