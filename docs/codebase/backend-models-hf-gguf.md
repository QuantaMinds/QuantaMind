# Backend — Model Management, Hugging Face, GGUF, Pull & VRAM

Subsystem doc for the model **installer + inspector**: how QuantaMind adds local
LLMs from three sources, parses GGUF binary headers, resumes/verifies downloads,
writes an Ollama "Modelfile" (chat template + params), and estimates VRAM/KV-cache
— file-by-file, scope-only.

Cross-links:
- The engine that **runs** an installed model: `backend-inference-backends.md`.
- The Models UI tab that drives these IPC commands: `frontend-models.md`.
- Disk/weights-dir accounting (`weights_dir`, `gguf_dest`, storage stats):
  `backend-prompt-workspace-system.md`.

---

## Overview

**Why.** Local-LLM users should never touch a terminal. The Models tab is one
modal with three tabs — **Ollama library pull**, **Hugging Face GGUF download**,
and **local `.gguf` file** — that all converge on the same registered-in-Ollama
end state, with one progress bar and typed failure popups. No `ollama pull`, no
`huggingface-cli`, no hand-written Modelfile.

**What.** Three install pipelines + a read-only inspector:

1. **Ollama library pull** — stream `POST /api/pull`, classify NDJSON status
   lines into phases, verify the manifest landed.
2. **Hugging Face GGUF** — search HF, browse repo files, download one `.gguf`
   with `.partial` resume, then import it into Ollama (create-from-blob).
3. **Local file** — copy the picked `.gguf` into the shared weights dir, inspect
   it, import into Ollama.
4. **Inspect** — read a GGUF binary header (arch/quant/ctx/params) *without*
   Ollama; read Ollama `/api/show` for template/capabilities/KV dims; compute the
   f16 KV-cache size from those dims.

**How (IPC command → handler file).**

| Command | File | Source / Purpose |
|---|---|---|
| `pull_model` / `cancel_pull` | `commands/models/models_pull.rs` | Ollama library pull (background task, cancel token) |
| `list_models` | `commands/models/models.rs` | `/api/tags` model names |
| `inspect_model` | `commands/models/model_inspect.rs` | `/api/show` template + caps + KV dims |
| `estimate_kv_cache_bytes` | `commands/models/model_inspect.rs` | KV-cache bytes from dims |
| `hf_search` | `commands/hf/hf_browse.rs` | HF repo search (gguf/mlx tag) |
| `hf_repo_files` / `hf_repo_all_files` | `commands/hf/hf_browse.rs` | repo `.gguf` list / full snapshot list |
| `hf_model_card` | `commands/hf/hf_card.rs` | structured model card |
| `install_hf_gguf` / `cancel_hf_install` | `commands/hf/hf_install.rs` | download HF GGUF → import |
| `inspect_gguf` | `commands/gguf/gguf_cmd.rs` | parse a `.gguf` header from disk |
| `install_local_gguf` | `commands/gguf/gguf_cmd.rs` | copy local file → import |

Emitted Tauri events: `pull-progress`, `hf-progress`, `local-install-progress`,
`models-changed`.

---

## `commands/models/` — Ollama pull + inspector commands

### File: `commands/models/models.rs`
- **Responsibility:** List installed Ollama models.
- **Why:** The Models tab and selectors need the live `/api/tags` list, with a
  short timeout so a hung Ollama doesn't freeze the UI.
- **What:** `list_models()` (IPC), `fetch_models(endpoint)`,
  `fetch_models_with_timeout(endpoint, timeout)`; private `TagsResponse`,
  `ModelEntry`, `map_request_err`. Names are sorted + deduped.
- **How/Where used:** `DEFAULT_TIMEOUT = 5s`; timeouts map to `AppError::Timeout`,
  other failures to `AppError::Inference`.

### File: `commands/models/model_inspect.rs`
- **Responsibility:** Model metadata for the inspector + the KV-cache predictor.
- **Why:** Template/capabilities/dims live only in Ollama's `/api/show`; for
  non-Ollama backends the command must say "Not available" rather than fabricate.
- **What:** `ModelDims`, `ModelInspect`, `inspect_model()` (IPC),
  `estimate_kv_cache_bytes()` (IPC), `fetch_dims(model)` (reused by the readiness
  VRAM-fit check), `classify_base()`, `dims_from_model_info()`.
- **How/Where used:** `dims_from_model_info` reads keys namespaced by
  `general.architecture` (e.g. `llama.block_count`). `head_count_kv` is the *only*
  tolerated absence — missing/null defaults to `head_count` (MHA) and flags
  `kv_estimated` so the derived figure is labelled a conservative overestimate,
  never silently wrong. The other four keys are required → `None`.

```rust
fn dims_from_model_info(info: &Map<String, Value>) -> Option<ModelDims> {
    let arch = info.get("general.architecture")?.as_str()?;
    let g = |s: &str| info.get(&format!("{arch}.{s}")).and_then(|v| v.as_u64());
    let head_count = g("attention.head_count")?;
    let kv = g("attention.head_count_kv");
    Some(ModelDims {
        layers: g("block_count")?,
        head_count,
        head_count_kv: kv.unwrap_or(head_count),   // MHA default
        kv_estimated: kv.is_none(),
        embedding_length: g("embedding_length")?,
        context_length: g("context_length")?,
    })
}
```

`classify_base` is advisory: a `tools` capability or chat-role markers
(`assistant`, `<|im_start`, `[inst]`, `<start_of_turn>`, …) ⇒ instruct; otherwise
"likely base" with a stated reason ("empty chat template" / "no chat-role markers";
"no 'tools' capability"). The UI states *why*, never an absolute claim.

### File: `commands/models/models_pull.rs`
- **Responsibility:** The Ollama-library pull command + cancellation registry.
- **Why:** Pulls are long-running and cancellable; the UI fires-and-forgets and
  listens for `pull-progress`. The pull must be panic-safe and verified.
- **What:** `PullState { active: Mutex<HashMap<pull_id, CancellationToken>> }`,
  `pull_model()` (IPC, returns a `pull_id` immediately), `cancel_pull()` (IPC).
- **How/Where used:** Spawns a tokio task that calls `inference::pull::pull`,
  emits each `PullProgress` as a `PullProgressEvent`. On success it does **not**
  trust Ollama — it calls `verify_model_registered` before broadcasting
  `models-changed`, because Ollama 0.24+ reports success before `/api/tags`
  reflects the manifest. The task is wrapped in `catch_unwind` so a panic becomes
  a `Failed` event, and the token is always removed from `active` on exit.

```rust
match AssertUnwindSafe(task).catch_unwind().await {
    Ok(Ok(())) => match verify_model_registered(DEFAULT_OLLAMA, &name_outer).await {
        Ok(()) => log_emit(&emit_app, EVENT_MODELS_CHANGED, ()),
        Err(e) => emit_failed(&emit_app, &pid, &name_outer, e.friendly()),
    },
    Ok(Err(e)) => emit_failed(&emit_app, &pid, &name_outer, e.friendly()),
    Err(panic)  => emit_failed(&emit_app, &pid, &name_outer,
                               format!("internal error: {}", panic_message(panic))),
}
```

### File: `commands/models/pull_events.rs`
- **Responsibility:** The wire shape + helpers for `pull-progress`.
- **What:** `EVENT_PULL_PROGRESS = "pull-progress"`,
  `PullProgressEvent { pull_id, name, progress: PullProgress }`,
  `panic_message(payload)` (downcasts `&str`/`String`), `emit_failed(...)` (emits a
  terminal `PullProgress::Failed`).
- **How/Where used:** Consumed only by `models_pull.rs`.

### File: `commands/models/mod.rs`
- Module wiring: `model_inspect`, `models`, `models_pull`, `pull_events`.

---

## `commands/hf/` — Hugging Face browse + GGUF install

### File: `commands/hf/hf_browse.rs`
- **Responsibility:** Thin IPC wrappers over the HF browse layer.
- **What:** `hf_search(query, limit?, kind?)`, `hf_repo_files(repo)` (GGUF list),
  `hf_repo_all_files(repo)` (full snapshot list). `HF_ENDPOINT =
  "https://huggingface.co"`, `DEFAULT_LIMIT = 30`, default `kind = Gguf`.
- **How/Where used:** Delegates to `inference::hf::hf_browse`. `#![deny(clippy::
  unwrap_used)]` at file top.

### File: `commands/hf/hf_card.rs`
- **Responsibility:** Fetch a repo's structured model card.
- **What:** `hf_model_card(repo) -> Option<ModelCard>` (delegates to
  `fetch_model_card`). `None` when the repo has no README.

### File: `commands/hf/hf_install.rs`
- **Responsibility:** The HF-GGUF install pipeline (download → import) with a
  single-in-flight guard and cancellation.
- **Why:** One HF install at a time; a cancel **or a failed download** must clean
  up both the final file and the `.partial` and free the single-install slot, so
  nothing broken lingers and the next attempt isn't blocked; the GGUF is kept for
  llama.cpp even if Ollama import is skipped.
- **What:** `HfInstallState { current: Mutex<Option<CancellationToken>> }` (slot
  shared with the MLX path via `.current()`), `install_hf_gguf()` (IPC),
  `cancel_hf_install()` (IPC), `install_hf_gguf_inner(...)`,
  `ollama_import_required(backend)`, `cleanup_incomplete_download(dest)`.
- **How/Where used:** `weights_dir` from user settings → `gguf_dest(dir, name)` is
  the destination. `download_gguf` streams the file emitting `HfPhase::Downloading`.
  On cancel **or download error**, `cleanup_incomplete_download` removes both `dest`
  and `partial_path(dest)` and the install slot is cleared (a download that errored
  leaves no resumable state — only an app-kill mid-stream leaves a `.partial` to
  resume). After a successful download, if Ollama is reachable it imports via
  `install_local_gguf_inner` (emitting `Hashing`/`Uploading`/`Installing`); if not
  reachable and the backend *requires* Ollama, it errors. On the import path the
  `.partial` marker is removed and the (complete) `dest` is kept for llama.cpp.

```rust
let dl = download_gguf(endpoint, repo, filename, &dest, on_dl, token.clone()).await;
if token.is_cancelled() {
    *state.current.lock_recover() = None;
    let _ = fs::remove_file(&dest);
    let _ = fs::remove_file(partial_path(&dest));
    return Err(AppError::Validation("install cancelled".into()));
}
dl?;
let result = if is_reachable(PROBE_TIMEOUT_MS).await {
    install_local_gguf_inner(DEFAULT_OLLAMA, &dest.to_string_lossy(), name, on_install).await
} else if ollama_import_required(backend) {
    Err(AppError::Inference("Ollama is not running — start it to add this model.".into()))
} else { Ok(()) };
```

### File: `commands/hf/hf_phase.rs`
- **Responsibility:** The `hf-progress` event shape.
- **What:** `EVENT_HF_PROGRESS = "hf-progress"`, `HfPhase` (tagged enum:
  `Downloading{bytes_completed,bytes_total,speed_bps}`, `Hashing{..}`,
  `Uploading{..}`, `Installing`), `HfPhase::from_create(CreatePhase)` — maps the
  local-create phases (`Hashing`/`Uploading`/`Creating`) onto the HF phases so one
  UI bar covers the whole download+import.

### File: `commands/hf/mod.rs`
- Module wiring: `hf_browse`, `hf_card`, `hf_install`, `hf_phase`.

---

## `commands/gguf/` — GGUF inspect + local install + verification

### File: `commands/gguf/gguf_cmd.rs`
- **Responsibility:** Inspect a `.gguf` and import a local/already-downloaded GGUF
  into Ollama (write the Modelfile via create-from-blob).
- **Why:** Local files and HF downloads both end here; importing must pick the
  right chat template and verify the manifest landed. Files are copied into the
  shared weights dir so llama.cpp can use them too.
- **What:** `EVENT_MODELS_CHANGED`, `EVENT_LOCAL_INSTALL_PROGRESS`,
  `retain_dest(dir,name,src)` (None when src already == dest, else the copy
  target), `inspect_gguf(path)` (IPC), `install_local_gguf_inner(endpoint, path,
  name, on_progress)`, `install_local_gguf()` (IPC).
- **How/Where used:** `install_local_gguf_inner` validates name + `.gguf` ext,
  fails fast if Ollama is down (before hashing a multi-GB file), inspects the
  header for the architecture, picks the template via
  `detect_template(name, Some(&meta.architecture))`, builds a `CreateSpec`, calls
  `ollama_create`, then `verify_model_registered`. `install_hf_install` reuses
  `install_local_gguf_inner`.

```rust
let meta = inspect(&p)?;
let canonical = p.canonicalize()?;
let spec = CreateSpec {
    gguf_path: canonical,
    chat_template: detect_template(name, Some(&meta.architecture)),
    parameters: CreateParameters::default(),
};
ollama_create(endpoint, name, &spec, on_progress).await?;
verify_model_registered(endpoint, name).await
```

### File: `commands/gguf/verify_install.rs`
- **Responsibility:** Confirm a freshly-created model actually appears in
  `/api/tags` before declaring success.
- **Why:** Ollama 0.24+ streams `{"status":"success"}` from `/api/create` 50–800ms
  *before* the manifest is visible; a one-shot check races and falsely reports a
  rollback.
- **What:** `verify_model_registered(endpoint, name)`,
  `verify_with_delays(endpoint, name, delays)` (`DELAYS_MS = [50,100,200,400,800,
  1500]` backoff), `has_model`, `tag_matches(tag, name)` (also matches the implicit
  `:latest`). Exhausting the backoff yields a precise `AppError::Inference`
  pointing at the Ollama server log.

### File: `commands/gguf/mod.rs`
- Module wiring: `gguf_cmd`, `verify_install`.

---

## `inference/hf/` — Hugging Face HTTP layer

### File: `inference/hf/hf_browse.rs`
- **Responsibility:** Search HF and list a repo's downloadable files.
- **Why:** Surface only repos with files the chosen backend can actually run; drop
  speech/audio GGUFs that can't run as an LLM.
- **What:** `RepoKind { Gguf, Mlx }` (+ `.tag()` / `.matches()`),
  `is_non_text_gguf(hit)`, `HfSearchHit`, `HfRepoFile`, `search_models()`,
  `fetch_tree()`, `repo_gguf_files()`, `repo_all_files()` (snapshot, minus
  `is_snapshot_junk`: `.gitattributes`, `*.md`, `license*`).
- **How/Where used:** `search_models` hits `GET /api/models` with `filter=<tag>`
  sorted by downloads, then post-filters by the same tag (belt-and-suspenders) and
  drops non-text GGUFs. `fetch_tree` hits `GET /api/models/{repo}/tree/main?
  recursive=true`. Sizes come straight from the tree entries (real, never
  estimated) so the UI shows a true download size.

```rust
fn matches(self, hit: &RawHit) -> bool {
    if !hit.tags.iter().any(|t| t.eq_ignore_ascii_case(self.tag())) { return false; }
    if self == RepoKind::Gguf && is_non_text_gguf(hit) { return false; }  // drop whisper/TTS
    true
}
```

### File: `inference/hf/hf_card.rs`
- **Responsibility:** Reduce a raw README into a structured `ModelCard`.
- **Why:** Real cards are arbitrary HTML; render structured fields + a few prose
  paragraphs, link out for the rest.
- **What:** `ModelCard { description, license, base_model, pipeline_tag, tags }`,
  `split_frontmatter(md)`, `extract_description(body)` (first ~3 prose paragraphs,
  skipping HTML/tables/headings via `is_prose`), `to_card(md)`,
  `fetch_model_card(endpoint, repo)`. YAML frontmatter is parsed with `serde_yaml`;
  `base_model` may be string-or-list (takes first). 404 → `Ok(None)`.

### File: `inference/hf/hf_request.rs`
- **Responsibility:** Shared HF request helpers.
- **What:** `validate_repo(repo)` (exactly `namespace/name`; ASCII alnum + `_-.`),
  `build_url(endpoint, repo, filename)` → `{endpoint}/{repo}/resolve/main/{file}`,
  `map_status(status, repo)` (404→`NotFound`, 401/403→`AuthRequired`,
  429/other→`Inference`), `build_client()` (streaming client).

### File: `inference/hf/hf_download.rs`
- **Responsibility:** Stream one repo file to disk with `.partial` resume,
  cancellation, and live speed.
- **Why:** Multi-GB downloads must survive interruption and over-read.
- **What:** `DownloadProgress { bytes_completed, bytes_total, speed_bps }`,
  `DownloadResult { final_path, sha256 }`, `parse_content_length(...)`,
  `download_gguf(...)` (guards `.gguf` ext), `download_file(...)` (format-agnostic;
  snapshots call it directly).
- **How/Where used:** If `dest` already exists → done. Otherwise `HEAD` for
  `Content-Length`, consult `decide(local_size(partial), total)` for a
  `ResumeStrategy`, send a `Range: bytes=N-` when resuming, append to `.partial`,
  abort if the server sends more bytes than `total`, then `rename` `.partial` →
  `dest`. Cancel returns early leaving the `.partial` for a later resume.

```rust
let mut req = client.get(&url);
if start > 0 { req = req.header("Range", format!("bytes={start}-")); }
...
loop { tokio::select! { biased;
    _ = cancel.cancelled() => return Ok(DownloadResult { final_path: partial.clone(), sha256: None }),
    chunk = bytes.next() => {
        let Some(chunk) = chunk else { break };
        let chunk = chunk?;
        if completed + chunk.len() as u64 > total { return Err(/* over-read */); }
        file.write_all(&chunk)?;
        completed += chunk.len() as u64;
        speed.add(Instant::now(), completed);
        on_progress(DownloadProgress { bytes_completed: completed, bytes_total: total,
                                       speed_bps: speed.bps(Instant::now()) });
    }
}}
```

### File: `inference/hf/hf_resume.rs`
- **Responsibility:** The resume decision (pure function over file sizes).
- **What:** `ResumeStrategy { Fresh, Resume(n), Skip, RedownloadAfterDelete }`,
  `decide(local, total)`, `partial_path(dest)` (appends `.partial`),
  `local_size(path)`.
- **How/Where used:** `decide`: no partial → `Fresh`; `n == total` → `Skip` (just
  rename); `n < total` → `Resume(n)`; `n > total` → `RedownloadAfterDelete`
  (corrupt/wrong file). This is the heart of resume correctness — tested directly.

```rust
pub fn decide(local: Option<u64>, total: u64) -> ResumeStrategy {
    match local {
        None                 => ResumeStrategy::Fresh,
        Some(n) if n == total => ResumeStrategy::Skip,
        Some(n) if n < total  => ResumeStrategy::Resume(n),
        Some(_)              => ResumeStrategy::RedownloadAfterDelete,
    }
}
```

### File: `inference/hf/hf_snapshot.rs`
- **Responsibility:** Download every file of a repo (MLX snapshot) into a dir,
  preserving nested paths, with an aggregate progress bar.
- **What:** `SnapshotProgress { bytes_completed, bytes_total, speed_bps,
  files_done, files_total }`, `download_snapshot(...)`.
- **How/Where used:** Sequential, per-file `.partial` resume via `download_file`;
  a finished file short-circuits, a cancel leaves partials for retry. `bytes_*`
  sum across all files so the UI shows a single running total. (Snapshot is the
  MLX-backend path; GGUF installs use the single-file `download_gguf`.)

### File: `inference/hf/mod.rs`
- Module wiring: `hf_browse`, `hf_card`, `hf_download`, `hf_request`, `hf_resume`,
  `hf_snapshot`.

---

## `inference/gguf/` — GGUF binary-header parser

The GGUF header is a little-endian binary block: magic `GGUF`, `u32` version,
`u64` tensor_count, `u64` kv_count, then `kv_count` key/value pairs. The app reads
the first 8 MiB and parses only the scalar metadata it needs, growing that window
on demand for models whose tokenizer metadata is larger (e.g. Qwen3).

### File: `inference/gguf/gguf_reader.rs`
- **Responsibility:** A bounds-checked little-endian cursor over a byte slice.
- **Why:** A malformed/truncated GGUF must error cleanly, never panic or read OOB.
- **What:** `GgufReader { bytes, pos }`; `take(n)` (overflow + truncation
  checked), typed readers `u8/u16/u32/u64/i16/i32/i64/f32/f64`, `magic(expected)`,
  `string()` (`u64` length-prefixed UTF-8, length validated against `usize`).
  Running off the end returns `AppError::Truncated` (not `Validation`), so the
  file-path inspector can tell "buffer too small, read more" from "invalid file".

```rust
fn take(&mut self, n: usize) -> Result<&'a [u8], AppError> {
    let end = self.pos.checked_add(n).ok_or_else(|| /* overflow */)?;
    if end > self.bytes.len() {
        return Err(AppError::Truncated(format!(
            "GGUF truncated: need {n} bytes at offset {}, have {}",
            self.pos, self.bytes.len() - self.pos)));
    }
    let out = &self.bytes[self.pos..end];
    self.pos = end;
    Ok(out)
}
```

### File: `inference/gguf/gguf_value.rs`
- **Responsibility:** Decode one typed GGUF metadata value; skip arrays.
- **Why:** Only scalar metadata is inspected, but every value (incl. arrays, which
  can be huge tokenizer tables) must be *drained* to keep the cursor aligned.
- **What:** `GgufValue` enum (scalars + `String` + `IntArray` + `ArraySkipped`),
  `read_value(r)` (tag-dispatched), `skip_value(r, tag)` (recursive for nested
  arrays), `checked_count`. Tags: 0..7 scalars, 8 string, 9 array (elem tag + u64
  count), 10..12 = u64/i64/f64. A **small int array** (≤ `MAX_CAPTURED_ARRAY`, e.g.
  gemma's per-layer `attention.head_count_kv`) is kept as `IntArray(Vec<i64>)`;
  string/float/nested and vocab-sized arrays are drained and discarded
  (`ArraySkipped`) so the cursor stays aligned without holding huge tables.

### File: `inference/gguf/gguf.rs`
- **Responsibility:** Top-level inspector: read the header from disk → typed
  `GgufMetadata`.
- **Why:** Show architecture/quant/context/params *before* importing, and feed the
  architecture into template detection — all without Ollama.
- **What:** `GgufMetadata { architecture, parameter_count, context_length,
  quantization, family }`, `inspect_gguf_bytes(bytes)`, `inspect_gguf(path)`,
  `as_string`/`as_u64` coercers. Starts at `HEADER_READ_BYTES = 8 MiB` (uses
  `take(N).read_to_end` because macOS `Read::read` only returns ~64KB per syscall);
  on `AppError::Truncated` it doubles the read window and retries, up to
  `MAX_HEADER_READ_BYTES = 256 MiB` (capped at file size) — so large-vocab
  tokenizers parse instead of falsely reporting a truncated file. A `Truncated`
  error once the window reaches that ceiling means a genuinely incomplete file.
  Rejects files under `MIN_FILE_SIZE = 64 KiB`. Supports GGUF versions 1–3.
- **How/Where used:** `quantization` comes from `general.file_type`; if absent it
  falls back to the filename. `context_length` is namespaced
  (`{arch}.context_length`). `inspect_gguf` is the `inspect_gguf` IPC + the
  local-install path.

```rust
r.magic(b"GGUF")?;
let version = r.u32()?;
if !(1..=3).contains(&version) { return Err(/* unsupported */); }
let _tensor_count = r.u64()?;
let kv_count = r.u64()?;
for _ in 0..kv_count {
    let key = r.string()?;
    kv.insert(key, read_value(&mut r)?);
}
let architecture = kv.get("general.architecture").and_then(as_string).unwrap_or("").to_string();
let context_length = kv.get(&format!("{architecture}.context_length")).and_then(as_u64)...;
let quantization = kv.get("general.file_type").and_then(/* U32 */).and_then(file_type_to_quant);
```

### File: `inference/gguf/gguf_quant.rs`
- **Responsibility:** Map quantization from the `general.file_type` enum or the
  filename.
- **What:** `file_type_to_quant(u32) -> Option<&str>` (0=F32, 1=F16, 7=Q8_0,
  15=Q4_K_M, 18=Q6_K, 32=BF16, …), `quant_from_filename(name)` (longest-match scan
  of `Q*_K_*`, `IQ*`, `BF16/F16/F32`, case-insensitive).
- **How/Where used:** `gguf.rs` prefers the header enum, falls back to the
  filename — so even GGUFs with an unrecognized `file_type` still show a quant.

### File: `inference/gguf/gguf_family.rs`
- **Responsibility:** Map an architecture string to a display family.
- **What:** `family_from_architecture(arch)` — `"llama"→"Llama"`,
  `"qwen2"→"Qwen 2"`, `"gemma2"→"Gemma 2"`, etc.; unknown arches pass through
  capitalized; `""` → `""`.

### File: `inference/gguf/mod.rs`
- Module wiring: `gguf`, `gguf_family`, `gguf_quant`, `gguf_reader`, `gguf_value`.

---

## `inference/pull/` — Ollama library pull engine

### File: `inference/pull/pull.rs`
- **Responsibility:** Stream `POST /api/pull` and drive a progress callback.
- **Why:** A pull is an NDJSON stream; lines must be parsed incrementally, the
  terminal `success` recognized even when Ollama 0.24+ omits the trailing newline,
  and cancellation honored mid-stream.
- **What:** `pull_model(endpoint, name, on_progress, cancel)`,
  `handle_line(line, speed, on_progress) -> bool` (true at `"success"`). Uses
  `ndjson::{next_line, tail}` for framing; `connect_timeout = 60s`; `SpeedTracker`
  over a 5s window. An `error` field in any chunk → `AppError::Inference`.

```rust
tokio::select! {
    _ = cancel.cancelled() => return Ok(()),
    piece = bytes.next() => {
        let Some(piece) = piece else { break };
        buf.extend_from_slice(&piece?);
        while let Some(line) = next_line(&mut buf) {
            if handle_line(&line, &mut speed, &mut on_progress)? { return Ok(()); }
            if cancel.is_cancelled() { return Ok(()); }
        }
    }
}
// parse un-flushed final `success` line on stream close:
if let Some(rest) = tail(&buf) { if handle_line(rest, ...)? { return Ok(()); } }
```

### File: `inference/pull/pull_progress.rs`
- **Responsibility:** The pull wire chunk + classification into typed phases.
- **What:** `PullProgress` (tagged enum, below), `PullRequest { name, stream }`,
  `PullChunk { status, error, digest, total, completed }`,
  `classify(chunk, speed_bps)`. A chunk with `digest+total+completed` ⇒
  `Downloading`; otherwise status string → manifest/verify/write/success;
  unrecognized → `None` (dropped with an eprintln).

### File: `inference/pull/pull_name.rs`
- **Responsibility:** Validate a model name (shared across all three pipelines).
- **What:** `validate_name(name)` — rejects empty/whitespace and any of
  `/ \ \0 " ' space tab newline`. Accepts `llama3.2:1b`,
  `qwen2.5-coder:7b-instruct-q4_K_M`.

### File: `inference/pull/pull_speed.rs`
- **Responsibility:** Moving-average download rate over a rolling window.
- **What:** `SpeedTracker { samples: VecDeque<(Instant, u64)>, window }`,
  `add(now, completed)` (evicts samples older than `window`), `bps(now)` (delta
  bytes / elapsed over the window; 0 with <2 samples). Reused by both the pull
  stream and the HF downloader.

### File: `inference/pull/mod.rs`
- Module wiring: `pull`, `pull_name`, `pull_progress`, `pull_speed`.

---

## `inference/create/` — Ollama "Modelfile" (create-from-blob)

Importing a GGUF means a `POST /api/create` with the file referenced by sha256
blob + an inline template/params object — the modern, Modelfile-free equivalent
of `FROM ./x.gguf` + `TEMPLATE` + `PARAMETER`.

### File: `inference/create/create_spec.rs`
- **Responsibility:** The typed inputs/outputs of a create.
- **What:** `CreateSpec { gguf_path, chat_template: Option<ChatTemplate>,
  parameters: CreateParameters }`, `CreateParameters { temperature, top_p, top_k,
  repeat_penalty, stop }` (all optional / default), `CreatePhase` (tagged enum:
  `Hashing{..}`, `Uploading{..}`, `Creating`).
- **How/Where used:** Built in `gguf_cmd.rs` with `detect_template(name, arch)`.

### File: `inference/create/create_body.rs`
- **Responsibility:** Serialize a `CreateSpec` into the `/api/create` JSON body.
- **Why:** This is where the chat template + stop tokens become the model's baked-in
  Modelfile, so a base GGUF chats correctly after import.
- **What:** `build_create_body(spec, model_name, digest)`,
  `parameters_to_json(params)`. Body = `{ model, files: { "<filename>":
  "sha256:<digest>" }, template?, parameters? }`. When a template is present its
  `stop_tokens` are merged ahead of any user stops.

```rust
body.insert("files".into(), json!({ &filename: format!("sha256:{digest}") }));
if let Some(t) = &spec.chat_template {
    body.insert("template".into(), json!(t.template_string));
    let mut stops: Vec<String> = t.stop_tokens.iter().map(|s| s.to_string()).collect();
    stops.extend(spec.parameters.stop.iter().cloned());
    if !stops.is_empty() { params.insert("stop".into(), json!(stops)); }
}
```

### File: `inference/create/consume_create.rs`
- **Responsibility:** Consume the `/api/create` NDJSON response.
- **What:** `consume_ndjson(resp)`, `consume_stream(stream)`, `handle_chunk(line,
  last_status)`; private `CreateChunk { status, error }`. Returns `Ok(())` only on
  a `"success"` chunk (incl. the newline-less remainder via `tail`); an `error`
  chunk → `AppError::Inference`; stream end without success → error quoting the
  last status seen.

### File: `inference/create/mod.rs`
- Module wiring: `consume_create`, `create_body`, `create_spec`.

> The actual `POST /api/create` (hash → blob-exists → upload → create → consume)
> lives in `inference/ollama/ollama_create.rs` — out of scope here; see
> `backend-inference-backends.md`. It calls `sha256_file`, `blob_exists`,
> `upload_blob`, `build_create_body`, `consume_ndjson`, emitting `CreatePhase`s.

---

## `inference/vram_math.rs`

- **Responsibility:** The single canonical f16 KV-cache size formula.
- **Why:** The frontend must show a real VRAM/KV figure from one source of truth,
  never re-implement it or fabricate.
- **What:** `calculate_kv_cache_bytes(layers, head_count, head_count_kv,
  embedding_length, context_length) -> u64`. `head_dim = embedding_length /
  head_count`; returns 0 when `head_count == 0` (no divide-by-zero on incomplete
  metadata). Verified: Llama-3-8B at 8K ctx = exactly 1 GiB.

```rust
pub fn calculate_kv_cache_bytes(layers, head_count, head_count_kv,
                                embedding_length, context_length) -> u64 {
    if head_count == 0 { return 0; }
    let head_dim = embedding_length / head_count;
    // 2 (K+V) × layers × kv_heads × head_dim × 2 (bytes/f16) × ctx
    2 * layers * head_count_kv * head_dim * 2 * context_length
}
```

Dims feeding this come from `inspect_model` (Ollama `/api/show`). When
`kv_estimated` is set (no `head_count_kv` in metadata, defaulted to MHA), the
figure is a labelled conservative overestimate for GQA models.

---

## Install-pipeline phases & emitted events

| Pipeline | Event | Phase variants (in order) | Carries |
|---|---|---|---|
| Ollama pull | `pull-progress` | `pulling_manifest` → `downloading` → `verifying` → `writing` → `success` / `failed` | `digest,total,completed,speed_bps`; `message` on fail |
| HF GGUF install | `hf-progress` | `downloading` → `hashing` → `uploading` → `installing` | `bytes_completed,bytes_total,speed_bps` |
| Local GGUF install | `local-install-progress` | `hashing` → `uploading` → `creating` | `bytes_completed,bytes_total` |
| MLX snapshot | (snapshot progress) | per-file aggregate | `bytes_*,speed_bps,files_done,files_total` |
| Any success | `models-changed` | — | `()` (UI refreshes `/api/tags`) |

`HfPhase::from_create` maps `CreatePhase::{Hashing,Uploading,Creating}` →
`HfPhase::{Hashing,Uploading,Installing}`, so the HF import reuses the local-create
phases under one bar.

---

## Data-flow: user picks an HF GGUF variant → download → verify → register

1. **Search.** UI calls `hf_search("qwen2.5", kind=Gguf)` →
   `search_models` hits `/api/models?filter=gguf&sort=downloads`, post-filters by
   tag, drops whisper/TTS GGUFs → `Vec<HfSearchHit>`.
2. **Browse.** UI calls `hf_repo_files(repo)` → `fetch_tree` (`/tree/main?
   recursive=true`) → only `.gguf` entries with real sizes → `Vec<HfRepoFile>`.
   `hf_model_card(repo)` shows license/base-model/description.
3. **Install.** UI calls `install_hf_gguf(repo, filename, name, backend)` →
   `install_hf_gguf_inner`: takes the single-in-flight token, computes
   `dest = gguf_dest(weights_dir, name)`.
4. **Download.** `download_gguf` → `HEAD` for `Content-Length`, `decide` resume
   strategy over `<dest>.partial`, stream with `Range` if resuming, abort on
   over-read, emit `hf-progress: downloading{...}`, `rename .partial → dest` on
   completion. Cancel leaves the `.partial` (resumable) and removes `dest`.
5. **Verify (file).** Implicit in the byte-exact `Content-Length` check + over-read
   guard; the rename only happens once bytes == total.
6. **Inspect + template.** If Ollama is reachable: `install_local_gguf_inner`
   inspects the GGUF header (`inspect_gguf` → architecture), picks
   `detect_template(name, arch)`, builds a `CreateSpec`.
7. **Register.** `ollama_create` hashes the GGUF → sha256 blob, uploads it if new
   (`hf-progress: hashing/uploading`), then `POST /api/create` with
   `build_create_body` (`files`+`template`+`parameters`) → `consume_ndjson` waits
   for `success` (`installing`).
8. **Verify (manifest).** `verify_model_registered` polls `/api/tags` with backoff
   until `name` (or `name:latest`) appears — defeats the Ollama 0.24+ success/tags
   race.
9. **Broadcast.** Emit `models-changed`; the UI re-runs `list_models`. The `.gguf`
   stays in the weights dir so llama.cpp can also load it.

If Ollama is **not** running: the GGUF is downloaded and kept; import is skipped
(success on non-Ollama backends) or errors with a "start Ollama" message on the
Ollama backend — a strong guardrail with a clear user-facing fix.
