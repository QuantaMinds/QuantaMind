# Frontend — Speech-to-Text (whisper.cpp axis)

> Subsystem doc. Scope: the **React/TS frontend** of QuantaMind's speech-to-text
> capability — the four feature folders `features/stt`, `features/sttWorkspace`,
> `features/sttEval`, `features/sttInspector`, plus the IPC modules under
> `shared/ipc/stt/*` and `shared/ipc/audio/*` they call. The Rust side (the
> whisper-server launcher, the transcribe pipeline, the scorer) is
> [`backend-stt.md`](./backend-stt.md) and is referenced, not duplicated.

## Overview

**Why STT is a separate axis.** A QuantaMind session runs **one LLM** (the
selected backend/model) *and* **one STT engine** (whisper.cpp) *in parallel*.
They are independent dimensions: the STT model is picked in its own header
dropdown, started/stopped by its own play/stop button, and its health is held in
its own store ([`sttRuntimeStore`](#state-stt)) — mirroring, never sharing, the
LLM backend's health flags. Speech-to-text is **whisper.cpp's `whisper-server`
on port 8093 only** (the `mlx-audio` engine was removed upstream; see project
memory). It is **offline by construction** — mic audio is captured natively in
Rust, decoded/resampled in Rust, and transcribed by a localhost server; nothing
reaches the cloud.

**What the voice → assistant loop is.** The headline STT product is a voice
pipeline **Audio → Transcript → LLM**:

1. The user **records** (native mic) or **uploads** an audio file.
2. Rust transcribes it; the canonical `Transcript` fills the transcript pane.
3. The transcript text becomes the **user message** to the selected LLM; an
   optional typed prompt is the **system/context** (e.g. *"You are a support
   agent"*). The LLM streams a reply.
4. **Auto-summarize** (a toggle) fires step 3 *automatically* the moment a
   transcription completes — so the end-to-end time (STT wall + LLM wall) is
   **production-faithful**. Manual **Ask** is labelled the same path but
   user-triggered (treated as optimistic timing — the user's think time isn't
   counted).

**How (the IPC commands).** Every capability is a Tauri `invoke`. The frontend
holds *no* audio bytes and *no* file contents — it passes **paths** and reads
back typed, Zod-validated results. The commands, grouped by surface:

| Group | Commands (frontend wrapper → Rust) |
| --- | --- |
| Engine/health | `check_whisper_env`, `check_whisper_health`, `start_whisper_server`, `stop_whisper_server` |
| Catalog/install | `list_stt_catalog`, `list_installed_stt_models`, `download_stt_model`, `cancel_stt_install`, `delete_stt_model` |
| Transcribe | `transcribe_audio`, `load_transcript` + events `stt-segments`, `stt-transcribe-progress` |
| Audio capture | `start_recording`, `stop_recording`, `recording_level` |
| Voice→LLM | `run_prompt`, `stop_prompt` + events `token`, `done`, `cancelled` (shared with the main Workspace) |
| Eval/readiness | `run_stt_eval`, `assess_stt_readiness`, `list_transcripts`, `list/load/save/delete_stt_eval`, `load_stt_report`, `list/save/delete_stt_readiness_profile` |

All Zod schemas live in `shared/ipc/stt/{stt,transcribe,eval}.ts` and
`shared/ipc/audio/capture.ts`. Every nullable field is honest: a metric the
backend didn't measure is `null` → renders **"N/A"** / **"Not available"**, never
a fabricated number (the *no-fake-metrics* rule).

---

## Surface map

Each STT surface, where it mounts, the components, the IPC it calls, and its
store.

| Surface | Mounts in | Components | IPC | Store |
| --- | --- | --- | --- | --- |
| **Header STT control** | top nav (always) | `SttHeaderControl` → `useSttServer` | `start/stop_whisper_server`, `check_whisper_health` | `sttRuntimeStore` (health), `sttSelectionStore` (model) |
| **Setup + catalog** | Models page → "Speech-to-Text" tab | `SpeechToTextTab`→`WhisperSttPanel`→{`SttSetupCard`,`SttCatalogTable`,`SttServerPanel`} | `check_whisper_env`, `list_stt_catalog`, `list_installed_stt_models`, `download_stt_model`, `cancel_stt_install` | `sttSelectionStore`, shared `modelStore` (download progress) |
| **Transcribe workspace** | `Workspace` (replaces LLM view when STT server up) | `SttWorkspace`→{`RecordControls`,`TranscriptPane`,`ReferencePane`,`VoiceAssistant`} | `start/stop_recording`, `recording_level`, `transcribe_audio` | `transcriptStore` |
| **Voice assistant** | inside `SttWorkspace` | `VoiceAssistant` → `useAssistantRun` | `run_prompt`/`stop_prompt` (events) | `assistantResultStore`, shared `compareStore` mirror |
| **STT eval panel** | Analysis tab | `SttEvalPanel`→{`SttEvalEditor`,`EvalReportTable`,`EvalVerdictTable`} | `run_stt_eval`, `assess_stt_readiness`, `list_transcripts`, spec/profile CRUD | local component state (no global store) |
| **STT inspector** | Latency page (`SttInspectorSection`) + Analysis tab (`SttAnalysisSection`) | charts/cards/phase-bar + `PipelineSummary` | none (reads stores) | `sttResultStore` (transcript), `assistantResultStore` (LLM stage) |

Mount points (verified):
`Workspace.tsx` switches to `<SttWorkspace/>` when `useSttRuntimeStore(runningSttEngine)`
is truthy; `ModelsPage.tsx` renders `<SpeechToTextTab/>` for the `stt` tab;
`InspectorPage.tsx` renders `<SttInspectorSection width={…}/>`; `AnalysisTab.tsx`
renders `<SttAnalysisSection/>` then `<SttEvalPanel/>`.

---

## `features/stt` — engine control, setup & install

The whisper.cpp axis itself: starting/stopping the one server, the engine-present
check, the curated model catalog, and the install flow.

### Hook: `useSttServer` — health-gated start/stop (IMPORTANT)

- **Responsibility:** own the lifecycle of the single `whisper-server` and keep a
  live `healthy` flag.
- **Why:** the header dot and the workspace auto-route both need to know *truly*
  whether the server is up — including a server someone started from the STT
  tab. A start that returns `started` doesn't mean the model finished loading, so
  the hook never trusts the return value for `healthy`; it trusts the **poll**.
- **What:** a 2s `check_whisper_health` poll drives `healthy` and mirrors it into
  `sttRuntimeStore`. `start()` branches on the **tagged** `SttStartResult` so each
  failure mode produces an actionable message (rendered through `SttError`);
  `started`/`already_running` leave `healthy` for the poll to flip.
- **How/where:** consumed by `SttHeaderControl` and `SttServerPanel` (same hook,
  two UIs, one server).

```ts
// poll keeps `healthy` live and shared (the spinner clears when the model loads)
const ok = (await checkWhisperHealth()).available;
setHealthy(ok); setShared(ok);      // sttRuntimeStore → Workspace auto-route
if (ok) setStarting(false);
// start: never set healthy from the return — branch on the tagged result
const r = await startWhisperServer(modelPath, vadPath);
if (r.status === "already_running" || r.status === "started") return; // poll flips it
if (r.status === "start_failed") setError(`${r.error}\n${r.stderr_tail}`);
else setError(r.note); // not_bundled | model_missing | vad_missing | port_conflict
```

### Components

| File | Responsibility / notes |
| --- | --- |
| **`SttHeaderControl`** | Global control: `[play/stop] ● Whisper.cpp [model ▾]`. Auto-selects the first installed model, calls `useSttServer.start(model_path, vad_path)`. Dot colour: gray=unknown, green=healthy, gray=down. Disables the model `<select>` while running/starting (can't swap model under a live server). Errors render in a popover via `SttError`. |
| **`SttServerPanel`** | The same start/stop + health dot **inside the Models tab** (below the catalog). Shares the global selection (`sttSelectionStore`), so picking a model here updates the header. Empty state nudges "download a model first". Mirrors `MlxServerControl`. |
| **`SpeechToTextTab`** | Thin wrapper — just renders `WhisperSttPanel`. |
| **`WhisperSttPanel`** | The 3-state engine gate from `useWhisperEnv`: not found → `SttSetupCard` (install); found-but-not-runnable → `SttSetupCard` (reinstall); ready → `SttCatalogTable` + `SttServerPanel`. |
| **`SttSetupCard`** | Amber, calm install/reinstall guide. Copies `brew install whisper-cpp` (or `brew reinstall …` when present-but-broken), a **Re-check** button, and a "choose its folder" escape hatch for a non-standard install. |
| **`SttCatalogTable`** | The curated catalog with Size + memory-**Fit** badge (real available RAM) before download. *No VRAM column* — VRAM is unmeasured for whisper.cpp, so it isn't faked. Active download progress/cancel/done render below, read from the shared `modelStore.downloads`. |
| **`SttError`** (`SttError.tsx`) | Turns a raw STT error into titled, step-by-step guidance via `sttGuidance(msg)` (dyld/library, not-installed, VAD-missing, model-missing, port-8093-conflict, unreachable, corrupt-download, start-failed). `lastLines(text, 8)` trims a verbose stderr tail to its root-cause lines. Unknown errors fall back to the plain message. Mirrors `ImportError`. |

### Hooks (non-server)

| File | Responsibility |
| --- | --- |
| **`useSttCatalog`** | Loads catalog + installed list (`Promise.all`), exposes `installedIds` set + `refresh`. Re-runs on the `models-changed` event so a freshly-downloaded model appears in the header dropdown without a reload. IPC errors are swallowed (early boot / non-Tauri) to avoid unhandled rejections. |
| **`useSttInstall`** | Downloads a whisper model **+ its shared VAD** as one op (`download_stt_model`), routed through the shared `modelStore` download bus (`source: "stt"`) so STT downloads appear on the Downloads page beside LLMs. `cancel()` → `cancel_stt_install`. Refreshes installed list on success. Mirrors `useMlxInstall`. |
| **`useWhisperEnv`** | `check_whisper_env` on mount + on-demand **Re-check** (busy-guarded against flicker). `chooseFolder()` opens a dir picker and persists `stt_engine_dir` to user settings so a manual install location survives launches. |

### <a id="state-stt"></a>State

| File | Shape |
| --- | --- |
| **`sttRuntimeStore`** | `whisperHealthy: boolean \| null`, written only by `useSttServer`'s poll. `runningSttEngine(s)` → `"whisper_cpp"` \| `null` is what `Workspace` reads to decide whether to swap in the STT view. Mirrors how `backendStore` holds LLM health. |
| **`sttSelectionStore`** | `selectedSttModelId: string \| null` — the **global** STT model selection, shared by header control + server panel. Its own axis, independent of the LLM. |

---

## `features/sttWorkspace` — the transcribe view & voice assistant

When the whisper server is healthy, `Workspace` swaps its LLM body for
`SttWorkspace` (see [`frontend-workspace.md`](./frontend-workspace.md)). The view
is `VoiceAssistant` over `RecordControls` over two panes (live transcript +
optional reference). On unmount it `reset()`s the transient transcript — **disk
is the source of truth**.

### Hook: `useMicRecorder` — native capture (IMPORTANT)

- **Responsibility:** drive native (Rust `cpal`) mic capture + a live level
  meter.
- **Why:** WKWebView's `getUserMedia` is unreliable in the Tauri macOS webview,
  so **audio never touches the webview** — Rust captures and returns a scratch
  WAV path on stop. macOS's mic-permission prompt fires on first
  `start_recording`; a denial records silence, surfaced later as
  `hadAudio=false`.
- **What:** `start()` → `start_recording` + a 100ms `recording_level` poll;
  `stop()` → `stop_recording` returning `{ path, hadAudio }`. Stop-without-start
  and double-stop are **no-ops** (return `null`). Level-poll failures are
  swallowed (cosmetic).

```ts
const start = async () => {
  if (recording) return;
  await startRecording(); setRecording(true);
  pollRef.current = setInterval(() => { recordingLevel().then(setLevel).catch(()=>{}); }, 100);
};
const stop = async (): Promise<RecordingResult | null> => {
  if (!recording) return null;             // no-op: stop-without-start / double-stop
  setRecording(false); stopPolling();
  const res = await stopRecording();
  return { path: res.path, hadAudio: res.had_audio };
};
```

### Hook: `useTranscription` — drive a transcription (IMPORTANT)

- **Responsibility:** subscribe to live segment/progress events and run a
  transcription against the running server.
- **Why:** segments stream in for a live pane, but the **persisted** transcript
  is canonical — so on completion the live view is replaced by the deduped truth
  returned from `transcribe_audio`, and a durable copy is parked for the
  Analysis/Latency sections.
- **What:** listeners for `stt-segments` (append) + `stt-transcribe-progress`
  (processed/total), torn down on unmount (no stacked listeners). `run(path)`
  resets, sets a `clip-<ts>` id, clears any prior LLM summary, calls
  `transcribe_audio`, then `loadFrom(transcript)` (live store) **and**
  `sttResultStore.setResult(transcript)` (durable).

```ts
const id = `clip-${Date.now()}`;
store.reset(); store.setCurrentId(id); store.setStatus("transcribing");
useAssistantResultStore.getState().clear();     // drop stale LLM summary
const transcript = await transcribeAudio(path, id);
useTranscriptStore.getState().loadFrom(transcript);    // reconcile live ← canonical
useSttResultStore.getState().setResult(transcript);    // durable for Latency
```

### Hook: `useAssistantRun` — the voice → LLM run (IMPORTANT)

- **Responsibility:** run the transcript through the selected LLM and capture the
  **measured** LLM-stage metrics.
- **Why:** the STT inspector needs the LLM half of the pipeline (TTFT,
  throughput, tokens, wall) rendered through the same rich path as a main
  Workspace run — without polluting the Workspace's history/compare flow.
- **What:** a purpose-built wrapper over the shared `run_prompt` event stream
  (`token`/`done`/`cancelled`). An `initiatedRef` guard makes the once-mounted
  listeners react **only to this hook's own run** (the stream is global — the
  main Workspace hook listens too). On `done` it computes wall time and writes
  the full result to `assistantResultStore` with `auto` from the run context.

```ts
const run = async (model, prompt, system?, ctx?) => {
  setStatus("running"); initiatedRef.current = true; startRef.current = performance.now();
  metaRef.current = { model, system: system?.trim() || null, ctx: ctx ?? { transcriptId: null, auto: false } };
  await invoke("run_prompt", { model, prompt, backend: selectedBackend, ...(system ? { system } : {}) });
};
// on EVENT_DONE (only if initiatedRef):
const wallMs = performance.now() - startRef.current;
useAssistantResultStore.getState().setResult({ transcriptId: meta.ctx.transcriptId, model: meta.model,
  ttftMs: done?.ttft_ms ?? null, tokensPerSec: done?.tokens_per_sec ?? null,
  tokenCount: done?.token_count ?? 0, totalMs: done?.stats?.total_ms ?? null, wallMs, auto: meta.ctx.auto });
```

### Component: `VoiceAssistant` — auto vs manual branch (IMPORTANT)

- **Responsibility:** the prompt box + Ask/Stop + **Auto-summarize** toggle that
  joins the transcript to the LLM.
- **What:** the joined transcript text is the user message; the typed prompt is
  the optional system/context. **Manual** Ask calls `run(..., { auto: false })`.
  **Auto** fires a `useEffect` once per fresh transcript (`lastAutoId` guard) the
  moment `status==="done"` — `{ auto: true }`, the production-faithful path. It
  also mirrors the run into `compareStore.setSingleRun` so the Analysis/Latency
  render the LLM stage through the same rich path as a SingleRun.

```ts
const onAsk = () => { if (model) void run(model, transcript, prompt, { transcriptId: currentId, auto: false }); };
useEffect(() => {                                  // the STT→LLM auto-pipe (fires once per clip)
  if (autoSummarize && ready && model && currentId && lastAutoId.current !== currentId && status !== "running") {
    lastAutoId.current = currentId;
    void run(model, transcript, prompt, { transcriptId: currentId, auto: true });
  }
}, [autoSummarize, ready, model, currentId, status, transcript, prompt, run]);
```

### Trivial components & state

| File | One line |
| --- | --- |
| **`SttWorkspace`** | Lays out `VoiceAssistant` / `RecordControls` / `[TranscriptPane | ReferencePane]`; `reset()`s the transcript on unmount. |
| **`RecordControls`** | Record ⇄ Stop + Upload; both converge to an audio path passed to `onRun`. Live level meter; on stop, `hadAudio=false` shows the "no audio detected — check mic/permission" notice instead of transcribing. Upload picker filters wav/mp3/m4a/flac/ogg. |
| **`TranscriptPane`** | Renders `Segment[]` directly (`m:ss` timestamp + text) — same shape as persisted, no divergent view. Shows % progress while transcribing; **Clear** when idle. |
| **`ReferencePane`** | Optional reference textarea; empty stays `null` in the store (first-class — drives reference-optional scoring), never coerced to `""`. |
| **`transcriptStore`** | Transient live state: `status`, `segments`, `reference`, progress, `currentId`, plus `stats`/`profile` of the finished run. `loadFrom(t)` reconciles to the canonical transcript; `reset` clears on unmount. |

---

## `features/sttEval` — scorer over stored transcripts

A **dumb, decoupled** eval surface: it scores the *already-stored* transcripts (it
does not re-transcribe). A spec joins a text instruction set (reference +
critical tokens) to a transcript **by id**; the backend computes WER and
behavioral metrics; a readiness profile turns metrics into a verdict. Lives on
the Analysis tab.

### `SttEvalPanel` — the surface

Holds spec list, profile list, the active report and verdicts. Two actions:
**Run eval** (`run_stt_eval(spec)` → `SttReport`) and **Assess readiness**
(`assess_stt_readiness(spec, profileId)` → ranked `SttModelVerdict[]`, gated on a
report existing). **+ New spec / Edit** open the editor. Errors via
`formatIpcError`. Empty state points at "+ New spec — or Generate starter".

### `SttEvalEditor` — spec builder

Per task: pick a **stored transcript** from a dropdown (no hunting for clip ids),
prefill its text as the reference (only when the reference is still empty),
comma/newline-split critical tokens. **Generate starter** makes one
self-referenced task per transcript so the panel works immediately. A blank
reference → `null` task (behavioral-only, "accuracy unverified"). Saves via
`saveSttEval(name, spec)`; lists transcripts via `list_transcripts`.

### `EvalReportTable` — per-(model,task) scores

One row per scored pair. Columns: Task, Model, RTF, **WER**, Weighted, Crit-acc,
Repeat, Silence, Conf, **Misreads** (`reference→heard` pairs). Every missing
metric → `"N/A"`; a task with no reference shows all WER columns `"N/A"`
(accuracy unverified), never a fabricated number.

### `EvalVerdictTable` — readiness verdict (IMPORTANT — WER gate)

Per-model card, **ranked best-first by the backend**. Status badge
`ready`/`conditional`/`not_ready`; the **gated figure is the weighted WER** (plus
raw WER and RTF); `blocking` (red) and `conditions` (amber) lists explain the
status verbatim from the backend.

```tsx
const s = STATUS[v.verdict.status] ?? { label: v.verdict.status, cls: "bg-gray-100" };
<span className={`… ${s.cls}`}>{s.label}</span>
<div>weighted WER {pct(v.weighted_wer)} · raw {pct(v.wer)} · RTF {v.rtf == null ? "N/A" : `${v.rtf.toFixed(2)}×`}</div>
{v.verdict.blocking.map((b,i) => <li key={i} className="text-red-600">{b}</li>)}   {/* hard fails */}
{v.verdict.conditions.map((c,i) => <li key={i} className="text-amber-700">{c}</li>)} {/* caveats */}
```

The eval contract (`SttEvalSpec`, `WerResult`, `SttReportRow`,
`SttModelVerdict`, readiness profiles) is defined in `shared/ipc/stt/eval.ts`.

---

## `features/sttInspector` — measured profile & pipeline metrics

Renders the **measured** profile of the last transcription with the same density
as the LLM Latency tab. Two entry points, both reading the **durable** stores
(`sttResultStore` = transcript, `assistantResultStore` = LLM stage) so they
survive leaving the Workspace; both auto-hide until a transcription exists.

- `SttInspectorSection` (Latency page): phase bar + confidence timeline +
  histogram + metric-card grid + pipeline one-liner.
- `SttAnalysisSection` (Analysis tab): the three headline metrics as ruler bars
  + transcript text + export + pipeline one-liner.

### Format: `confidenceTimeline` — the segment transform (IMPORTANT)

Pure. Maps `Segment[]` → per-segment bars over the audio timeline. `confidence`
is `exp(avg_logprob)` clamped to 0..1, or **`null` when the backend emitted no
logprob** (rendered as a gap, never a guessed 0). A segment is `silenceOut` when
whisper flags non-speech yet emitted text (hallucination risk), else `low` below
the decode-failure gate, else `ok`. Thresholds are **whisper's own** gates
(`avg_logprob < -1.0`, `no_speech_prob > 0.6`) — stable and interpretable, not a
per-run robust threshold that would mislabel the worst segment of a clean clip.

```ts
const confidence = s.avg_logprob == null ? null : clamp01(Math.exp(s.avg_logprob)); // null = gap, no fake 0
const isSilenceOut = s.no_speech_prob != null && s.no_speech_prob > NO_SPEECH_THRESHOLD && s.text.trim() !== "";
const isLow = s.avg_logprob != null && s.avg_logprob < LOW_LOGPROB;
const kind = isSilenceOut ? "silenceOut" : isLow ? "low" : "ok";
// audioSecs = max(declared duration, last segment end) so bars never overflow
```

### Format & charts (other)

| File | One line |
| --- | --- |
| **`confidenceHistogram`** (format) | Buckets measured confidences into ≤10 equal-width bins over [0,1]; a bin holding any flagged segment is marked; `[]` for <2 measured points (no distribution). Pure. |
| **`sttMetrics`** (format) | `wordCount` (real whitespace tokens) and `wordsPerSec` = words ÷ wall seconds; `null` when wall is missing/zero or no words (no fake 0). |
| **`sttReport`** (format) | `toSttJson` / `toSttMarkdown` — self-contained report (derived metrics + raw segments). Every unmeasured field → "N/A"; VRAM is "Not available for this backend". |
| **`ConfidenceTimeline`** (visx) | x=audio time, y=confidence; null-confidence segments are gaps; low/silenceOut bars coloured (blue/red/orange) as scrutiny markers; hover readout per segment. |
| **`ConfidenceHistogram`** (visx) | x=confidence bin, y=segment count; flagged bins rose; hover readout. |
| **`SttPhaseBar`** | `[ first-segment latency | rest of transcription ]` across `0→transcribe_wall_ms` (the only honest phases whisper-server reports). N/A note when no wall time. |
| **`SttMetricCards`** | Big-number grid (RTF, first-segment, words/sec, segments, audio dur, wall, mean confidence, repeat rate, silence-output rate, language); each missing cell → "N/A"; VRAM card always "Not available for this backend". |
| **`SttAnalysisSection`** | Three ruler bars (RTF / words-per-sec / first-segment) + transcript text + export buttons + pipeline one-liner; null → "Not available". |
| **`SttExportButtons`** | Export the transcript as MD/JSON via the generic `save_compare_report` file-writer. |

### `PipelineSummary` — the end-to-end one-liner

**Audio → Transcript → LLM summarize · end-to-end**, each stage's measured time.
**End-to-end = STT wall + LLM wall** (processing time, *not* the audio length).
Renders **only when the LLM stage's `transcriptId` matches the shown transcript**
— so the two stages are never displayed mismatched. An **auto** badge marks an
auto-summarized (production-faithful) run.

### State

| File | Shape |
| --- | --- |
| **`sttResultStore`** | Durable `result: Transcript \| null` — survives tab nav (unlike the transient `transcriptStore`). Set by `useTranscription`. Mirrors `compareStore` for LLM runs. |
| **`assistantResultStore`** | Durable `AssistantResult` for the LLM stage: `transcriptId` (ties it to the STT result), measured `ttftMs`/`tokensPerSec`/`tokenCount`/`totalMs`/`wallMs`, and `auto`. Every metric nullable → "N/A". |

---

## Data-flow walkthrough

End-to-end, the happy path through every surface:

1. **Install a model.** Models page → *Speech-to-Text* tab. `useWhisperEnv` →
   `check_whisper_env`; if missing, `SttSetupCard` walks `brew install`. Once
   runnable, `SttCatalogTable` lists the catalog; **Download** →
   `useSttInstall` → `download_stt_model` (model + VAD), progress on the shared
   download bus. `useSttCatalog` refreshes on `models-changed`.
2. **Start whisper-server.** Header (or `SttServerPanel`) → `useSttServer.start(model_path, vad_path)`
   → `start_whisper_server` on **:8093**. The 2s health poll flips the dot green
   and writes `sttRuntimeStore.whisperHealthy = true`.
3. **Workspace switches to the transcribe view.** `Workspace` reads
   `runningSttEngine` and renders `<SttWorkspace/>` in place of the LLM view.
4. **Record / upload.** `RecordControls` → `useMicRecorder` (`start/stop_recording`,
   `recording_level`) or the file picker → an audio **path**.
5. **Transcribe.** `useTranscription.run(path)` → `transcribe_audio`. Segments
   stream via `stt-segments` into `TranscriptPane`; on completion the canonical
   `Transcript` reconciles the live view and lands in `sttResultStore`.
6. **(Auto-summarize) LLM reply.** `VoiceAssistant`'s effect fires
   `useAssistantRun.run(..., { auto: true })` → `run_prompt`; the reply streams,
   and the measured LLM metrics land in `assistantResultStore`.
7. **Latency metrics.** `SttInspectorSection` / `SttAnalysisSection` render the
   confidence timeline/histogram, phase bar, metric cards, and the
   `PipelineSummary` end-to-end one-liner. Export to MD/JSON.
8. **(Later) Eval.** Analysis tab → `SttEvalPanel`: build a spec over stored
   transcripts (`SttEvalEditor`), **Run eval** → `EvalReportTable`, **Assess** →
   `EvalVerdictTable` (weighted-WER-gated verdict).

---

## Cross-links

- [`backend-stt.md`](./backend-stt.md) — the Rust whisper-server launcher,
  transcribe pipeline (decode/resample), scorer, and the IPC commands' impl.
- [`frontend-workspace.md`](./frontend-workspace.md) — the main Workspace whose
  body the transcribe view replaces when the STT server is up.
- [`frontend-overview.md`](./frontend-overview.md) — the STT axis in the wider
  app (header controls, tabs, shared IPC/Zod conventions).
- [`backend-persistence.md`](./backend-persistence.md) — disk-truth for
  transcripts (`load_transcript`), eval specs, reports, and readiness profiles.
