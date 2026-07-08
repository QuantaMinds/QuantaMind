import { Fragment, useEffect, useRef, useState } from "react";
import { modelLabel } from "../../../shared/models/modelLabel";
import { useSelectedModelStore } from "../../../shared/state/selectedModelStore";
import { useParamsStore } from "../../../shared/state/paramsStore";
import { useEvalRegistryStore, DEFAULT_PRESET } from "../state/evalRegistryStore";
import { PresetOptGroups } from "./PresetOptGroups";
import { getBuiltinCollection, loadCustomCollection, type ToolTask } from "../../../shared/ipc/eval/registry";
import { useVramFit } from "../../quant/useVramFit";
import { useHardwareSnapshot } from "../../models/hooks/useHardwareSnapshot";
import { loadedModels, type LoadedModel } from "../../../shared/ipc/system/vram";
import { formatBytes } from "../../../shared/format/bytes";
import { useCliffStore } from "../state/cliffStore";
import { InfoButton } from "../../../shared/ui/InfoButton";
import { TOOL_HELP, METRIC_HELP } from "../help";
import { classifyCliff } from "../cliff";
import { ContextCliffChart } from "./ContextCliffChart";
import type { BackendKind } from "../../../shared/ipc/models/storage";
import type { AgentPath } from "../../../shared/ipc/eval/readiness";
import type { CliffPreset } from "../../../shared/ipc/eval/cliff";

interface ProbeModel {
  name: string;
  backend: BackendKind;
  /// GGUF path for llama.cpp models (carried through from `SelectedModel`); the probe
  /// sends it so the backend can match the running llama-server. Absent for Ollama/MLX.
  path?: string;
}

const FALLBACK_MAX_TOKENS = 65536; // slider ceiling when the model context window is unknown

/// Context-Cliff probe: runs a chosen dataset at growing prompt lengths and
/// graphs where tool-call accuracy collapses. Frontend-only, padding is
/// approximate (≈tokens) — labelled indicative, not a tokenizer. Owns its own
/// collection selection so it never depends on the EvalManager editor.
export function ContextCliffPanel() {
  const { presets, collections, init } = useEvalRegistryStore();
  const [active, setActive] = useState(DEFAULT_PRESET);
  const [tasks, setTasks] = useState<ToolTask[]>([]);
  const [maxTokens, setMaxTokens] = useState(16384);
  const [testSteps, setTestSteps] = useState(5);
  // Which embedded synthetic preset fills the context (the backend cycles it to each
  // verified depth, char-boundary-safe). The probe is always greedy (temp 0) — a
  // diagnostic must reproduce — so the backend pins it; there is no local toggle.
  const [preset, setPreset] = useState<CliffPreset>("corporate_policy");
  // Which tool-calling path the probe runs — the USER picks it on this page (default native, like
  // the batch). MLX has no native tool API, so native falls back to prompt-based there.
  const [method, setMethod] = useState<AgentPath>("native_fc");
  // The probe runs ONE of the global header models + global params. With 2+
  // selected (Ollama), a small dropdown picks which one; default the first. A
  // pre-fill request from the Matrix can OVERRIDE that with any batch-target model.
  const selectedModels = useSelectedModelStore((s) => s.selectedModels);
  const globalParams = useParamsStore((s) => s.globalParams);
  const [probeName, setProbeName] = useState("");
  const [override, setOverride] = useState<ProbeModel | null>(null);
  // Which rung's per-step trace (system prompt + outputs) is expanded, by row index.
  const [openTrace, setOpenTrace] = useState<number | null>(null);
  const selected: ProbeModel | null =
    override ?? selectedModels.find((m) => m.name === probeName) ?? selectedModels[0] ?? null;
  const model = selected?.name ?? "";
  // Header models, plus the Matrix-pre-filled override when it isn't one of them.
  const modelOptions: ProbeModel[] =
    override && !selectedModels.some((m) => m.name === override.name) ? [override, ...selectedModels] : selectedModels;

  // Keep the probe model inside the current selection (e.g. after a backend switch),
  // unless an explicit Matrix pre-fill override is in effect.
  useEffect(() => {
    if (!override && probeName && !selectedModels.some((m) => m.name === probeName)) setProbeName("");
  }, [selectedModels, probeName, override]);

  // ── Cliff store: the probe run lives in the store so it survives navigation ──
  const points = useCliffStore((s) => s.points);
  const running = useCliffStore((s) => s.running);
  const error = useCliffStore((s) => s.error);
  const progress = useCliffStore((s) => s.progress);
  const frac = useCliffStore((s) => s.frac);
  const step = useCliffStore((s) => s.step);
  const startedAt = useCliffStore((s) => s.startedAt);
  const runningModel = useCliffStore((s) => s.runningModel);
  const runProbe = useCliffStore((s) => s.runProbe);
  const stopProbe = useCliffStore((s) => s.stop);
  const resetProbe = useCliffStore((s) => s.reset);
  const request = useCliffStore((s) => s.request);
  const consumeRequest = useCliffStore((s) => s.consumeRequest);

  // Consume a Matrix pre-fill REACTIVELY (keyed on `request`), not once-on-mount: the
  // Audit page is always-mounted-and-hidden, so a mount-only effect would fire once at
  // app load (request null) and never again. Watching `request` means a "Run probe"
  // click ALWAYS lands the panel pre-filled with model + collection + max-tokens +
  // steps. consumeRequest() nulls it → this re-runs with null and no-ops (no loop).
  // NEVER auto-runs (guardrail 1) — the user clicks Execute.
  useEffect(() => {
    if (!request) return;
    // Carry the GGUF `path` into the override — without it the llama.cpp probe sends an empty
    // path and the backend falsely reports "WrongModel" ("Start llama.cpp with …").
    setOverride({ name: request.model, backend: request.backend, path: request.path });
    setActive(request.collectionId);
    setMaxTokens(request.maxTokens);
    setTestSteps(request.steps);
    consumeRequest();
  }, [request, consumeRequest]);

  const isPreset = (id: string) => presets.some((p) => p.id === id);

  useEffect(() => {
    void init().catch((e) => console.error("eval registry init failed (ContextCliffPanel):", e));
  }, [init]);

  // Load the chosen collection's tasks itself (preset OR custom) so the probe
  // always has a real dataset to run — independent of the editor.
  useEffect(() => {
    if (presets.length === 0) return;
    let cancelled = false;
    (async () => {
      try {
        const t = isPreset(active) ? await getBuiltinCollection(active) : await loadCustomCollection(active);
        if (!cancelled) setTasks(t);
      } catch {
        if (!cancelled) setTasks([]);
      }
    })();
    return () => { cancelled = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, presets]);

  // Cap the padding ladder at the model's real context window when known
  // (Ollama /api/show dims); fall back to a fixed ceiling otherwise.
  const { dims, kvBytes } = useVramFit(selected?.name, selected?.backend, maxTokens);
  const sliderMax = dims?.context_length ? Math.max(4096, dims.context_length) : FALLBACK_MAX_TOKENS;
  // Default Max Tokens to the model's FULL context window once it's known — a model's
  // cliff can sit anywhere up to its real window, so the probe should sweep the whole
  // thing by default (Run probe ↗ lands here pre-filled). Done once per model so a
  // manual slider change is never clobbered; you can still dial it down for speed.
  const defaultedFor = useRef<string | null>(null);
  useEffect(() => {
    const m = selected?.name ?? null;
    if (m && dims?.context_length && defaultedFor.current !== m) {
      setMaxTokens(sliderMax);
      defaultedFor.current = m;
    }
  }, [selected?.name, dims?.context_length, sliderMax]);
  // Keep the value within the window after a model switch (a smaller window clamps down).
  useEffect(() => {
    setMaxTokens((m) => Math.min(m, sliderMax));
  }, [sliderMax]);

  const verdict = classifyCliff(points);
  const cliff = verdict.kind === "cliff" ? verdict.depth : null;

  // Clear a stale chart when the selection changes (and nothing is running), so the
  // graph always reflects the currently-selected model/collection.
  useEffect(() => {
    if (!running) resetProbe();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [model, active]);

  const maintainedTo = points.reduce(
    (mx, p) => (p.composite != null && p.promptTokens != null && p.promptTokens > mx ? p.promptTokens : mx),
    0,
  );
  const lastDepth = points.length > 0 ? points[points.length - 1].promptTokens : null;

  // A 1 Hz clock, live only while probing, so the elapsed/ETA readout ticks without
  // re-rendering the whole panel when idle.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!running) return;
    setNow(Date.now());
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [running]);

  // Overall completion fraction across the WHOLE ladder. The store owns it (see
  // `progressFraction` in cliffStore): rung boundaries anchored on the authoritative
  // per-rung counter, within-rung fill capped below the boundary, and kept MONOTONIC so a
  // verify-and-adjust re-sweep never drives the bar backward or falsely claims 100%.
  const elapsedS = running && startedAt != null ? Math.max(0, (now - startedAt) / 1000) : 0;
  // Linear extrapolation from elapsed ÷ fraction — only once there's enough signal to not
  // show a wild first guess. It's an estimate, labelled "~", never presented as exact.
  const etaS = frac > 0.03 && frac < 1 && elapsedS > 3 ? (elapsedS * (1 - frac)) / frac : null;

  // Pre-flight memory advisory. Ollama sizes num_ctx per request; the backend hard-stops a
  // won't-fit run, this warns BEFORE the click so the user can dial Max Tokens down first.
  // Device cap = unified → system RAM, discrete → VRAM. Estimate = model weights (from a
  // loaded Ollama model) + KV cache at the requested depth. Renders nothing unless it can be
  // MEASURED (cap + KV both present) — never a guessed alarm. Ollama only (where weights are
  // readable); llama.cpp/MLX have their own guards.
  const { snapshot } = useHardwareSnapshot();
  const [loaded, setLoaded] = useState<LoadedModel[]>([]);
  useEffect(() => {
    let cancelled = false;
    loadedModels()
      .then((l) => !cancelled && setLoaded(l))
      .catch(() => !cancelled && setLoaded([]));
    return () => {
      cancelled = true;
    };
  }, [selected?.name]);
  const deviceCap: number | null = snapshot?.gpu?.unified
    ? snapshot.total_memory_bytes
    : snapshot?.gpu?.vram_total_bytes ?? null;
  const weightsBytes = loaded.find((m) => m.name === selected?.name)?.size_bytes ?? null;
  const neededCtxK = Math.round((maxTokens + 2048) / 1000); // mirrors the backend CLIFF_CTX_HEADROOM
  const footprint = kvBytes != null ? (weightsBytes ?? 0) + kvBytes : null;
  const fitWarning: string | null =
    selected?.backend === "ollama" && deviceCap != null && footprint != null && footprint > deviceCap * 0.85
      ? footprint > deviceCap
        ? `This machine (${formatBytes(deviceCap)}) likely can't hold ~${neededCtxK}k tokens for ${selected.name} — needs ≈${formatBytes(footprint)}. Reduce Max Tokens (or use a smaller model/quant) before running.`
        : `High memory pressure: ~${neededCtxK}k tokens for ${selected.name} needs ≈${formatBytes(footprint)} of ${formatBytes(deviceCap)} — close to the limit, so the run may spill to CPU (slow).`
      : null;

  // MLX has no native tool-calling API, so native isn't offered there — the run always uses the
  // effective method (native falls back to prompt-based on MLX).
  const nativeAvailable = selected?.backend !== "mlx";
  const effectiveMethod: AgentPath = nativeAvailable ? method : "prompt_based";

  const handleRun = () => {
    if (!selected) return;
    setOpenTrace(null); // a fresh run rebuilds the rungs — drop any expanded trace
    void runProbe({
      model: selected.name,
      backend: selected.backend,
      collectionId: active, // scope the saved cliff per (collection, model)
      tasks,
      maxTokens,
      steps: testSteps,
      source: { kind: "preset", preset },
      params: globalParams,
      modelPath: selected.path, // llama.cpp: backend matches it to the running server
      method: effectiveMethod, // native vs prompt-based — the user's choice on this page
    });
  };
  const handleStop = () => stopProbe();
  const handleReset = () => resetProbe();

  return (
    <div
      className="rounded-xl overflow-hidden border border-slate-200 bg-white"
      style={{
        boxShadow: "0 1px 3px rgba(0,0,0,0.05)",
      }}
      data-testid="cliff-panel"
    >
      {/* ── Header ── */}
      <div className="flex items-center justify-between px-5 pt-5 pb-3">
        <div>
          <h2
            className="text-lg font-semibold tracking-tight"
            style={{ color: "#1e293b", fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif" }}
          >
            Context Stress Test
          </h2>
          <select
            value={active}
            onChange={(e) => { setActive(e.target.value); setOverride(null); resetProbe(); }}
            data-testid="cliff-collection-select"
            style={{
              background: "transparent",
              border: "none",
              color: "#64748b",
              fontSize: 12,
              fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
              outline: "none",
              cursor: "pointer",
              padding: 0,
              marginTop: 3,
            }}
          >
            <PresetOptGroups presets={presets} />
            {collections.map((c) => <option key={c} value={c}>{c}</option>)}
          </select>
        </div>
        <div className="flex items-center gap-2">
          <InfoButton {...TOOL_HELP.contextCliff} testId="context-cliff" />
          <button
            type="button"
            onClick={handleReset}
            title="Reset"
            style={{
              background: "#f1f5f9",
              border: "1px solid #cbd5e1",
              color: "#475569",
              width: 36,
              height: 36,
              borderRadius: "50%",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              fontSize: 16,
              cursor: "pointer",
              flexShrink: 0,
              transition: "background 0.15s",
            }}
            onMouseEnter={(e) => (e.currentTarget.style.background = "#e2e8f0")}
            onMouseLeave={(e) => (e.currentTarget.style.background = "#f1f5f9")}
          >
            ↺
          </button>
        </div>
      </div>

      {/* ── Error (a backend failure is shown, never a silent blank chart) ── */}
      {error && (
        <div style={{ padding: "0 20px 8px" }}>
          <p
            style={{
              fontSize: 11,
              color: "#dc2626",
              fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
              background: "rgba(220,38,38,0.05)",
              border: "1px solid rgba(220,38,38,0.15)",
              borderRadius: 6,
              padding: "6px 10px",
            }}
            data-testid="cliff-error"
          >
            Not available — {error}
          </p>
        </div>
      )}

      {/* ── Chart ── */}
      <div className="px-4 pb-2">
        {points.length > 0 ? (
          <ContextCliffChart points={points} width={580} height={220} />
        ) : (
          <div
            style={{
              width: 580,
              height: 220,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              color: "#94a3b8",
              fontSize: 13,
              fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
              letterSpacing: "0.02em",
            }}
          >
            {running ? "Probing…" : "Run the probe to see results"}
          </div>
        )}
      </div>

      {/* ── Results Table ── */}
      {points.length > 0 && (
        <div className="px-5 pb-3">
          <table style={{ width: "100%", borderCollapse: "separate", borderSpacing: 0 }}>
            <thead>
              <tr>
                {[
                  { label: "Step" },
                  { label: "Tokens" },
                  { label: "Accuracy", help: METRIC_HELP.cliffAccuracy },
                  { label: "Status" },
                  { label: "Trace" },
                ].map((h) => (
                  <th
                    key={h.label}
                    style={{
                      textAlign: "left",
                      padding: "6px 12px",
                      fontSize: 12,
                      fontWeight: 600,
                      color: "#64748b",
                      fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
                      borderBottom: "1px solid #e2e8f0",
                      letterSpacing: "0.03em",
                    }}
                  >
                    <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
                      {h.label}
                      {h.help && <InfoButton {...h.help} testId="cliff-accuracy" />}
                    </span>
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {points.map((p, i) => {
                const pct = p.composite != null ? `${(p.composite * 100).toFixed(1)}%` : "—";
                const passed = p.composite != null && p.composite >= 0.5;
                const failed = p.composite != null && p.composite < 0.5;
                const isEven = i % 2 === 0;
                const traceCount = p.trace?.length ?? 0;
                const open = openTrace === i;

                return (
                  <Fragment key={i}>
                    <tr style={{ background: isEven ? "#f8fafc" : "transparent" }}>
                      <td style={tdStyle}>{i + 1}</td>
                      <td style={tdStyle}>
                        {p.promptTokens != null ? Math.round(p.promptTokens).toLocaleString() : "Not available"}
                      </td>
                      <td style={{ ...tdStyle, fontWeight: 600, color: "#1e293b" }}>{pct}</td>
                      <td style={tdStyle}>
                        {passed && (
                          <span style={passChipStyle}>
                            Pass
                          </span>
                        )}
                        {failed && (
                          <span style={failChipStyle}>
                            Failure
                          </span>
                        )}
                        {p.composite == null && (
                          <span style={{ color: "#64748b", fontSize: 12 }}>Error</span>
                        )}
                      </td>
                      <td style={tdStyle}>
                        {traceCount > 0 ? (
                          <button
                            type="button"
                            onClick={() => setOpenTrace(open ? null : i)}
                            data-testid={`cliff-trace-toggle-${i}`}
                            aria-expanded={open}
                            style={traceBtnStyle}
                          >
                            {open ? "Hide trace" : "View trace"}
                          </button>
                        ) : (
                          <span style={{ color: "#cbd5e1", fontSize: 12 }}>—</span>
                        )}
                      </td>
                    </tr>
                    {open && traceCount > 0 && (
                      <tr data-testid={`cliff-trace-${i}`}>
                        <td colSpan={5} style={{ padding: "4px 12px 12px", background: "#f1f5f9" }}>
                          {(p.trace ?? []).map((t, ti) => (
                            <div key={ti} style={traceCardStyle}>
                              <div style={traceTaskStyle}>{t.task_id}</div>
                              {t.outputs.map((o, oi) => (
                                <div key={oi} style={{ marginTop: 8 }}>
                                  <div style={traceLabelStyle}>
                                    {/* Rung 0 is the unpadded baseline — no padding is injected there. */}
                                    {i === 0 ? "Unpadded baseline" : `Needle at ${Math.round(o.depth * 100)}%`}{" "}
                                    <span style={o.passed ? passChipStyle : failChipStyle}>{o.passed ? "Pass" : "Failure"}</span>
                                  </div>
                                  <div style={traceSubLabelStyle}>{i === 0 ? "Input (no padding)" : "Padded input (context + needle)"}</div>
                                  <pre style={tracePreStyle}>{(o.prompt ?? "").trim() === "" ? "(none)" : o.prompt}</pre>
                                  <div style={{ ...traceSubLabelStyle, marginTop: 6 }}>Output</div>
                                  <pre style={tracePreStyle}>{o.output.trim() === "" ? "(empty output)" : o.output}</pre>
                                </div>
                              ))}
                            </div>
                          ))}
                        </td>
                      </tr>
                    )}
                  </Fragment>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {/* ── Divider ── */}
      <div style={{ height: 1, background: "#f1f5f9", margin: "0 0" }} />

      {/* ── Model & Status Row ── */}
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "1fr 1fr",
          padding: "14px 20px",
          borderBottom: "1px solid #f1f5f9",
        }}
      >
        <div>
          <div
            style={{
              fontSize: 11,
              color: "#64748b",
              marginBottom: 4,
              fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
              letterSpacing: "0.04em",
              textTransform: "uppercase",
              fontWeight: 650,
            }}
          >
            Model
          </div>
          <div data-testid="cliff-model" style={{ fontSize: 13, color: "#475569", fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif" }}>
            {modelOptions.length >= 2 ? (
              <select
                value={model}
                onChange={(e) => {
                  setOverride(null);
                  setProbeName(e.target.value);
                }}
                data-testid="cliff-model-select"
                style={{
                  background: "#ffffff",
                  border: "1px solid #cbd5e1",
                  color: "#334155",
                  borderRadius: 6,
                  padding: "3px 8px",
                  fontSize: 12,
                  fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
                  outline: "none",
                  cursor: "pointer",
                }}
              >
                {modelOptions.map((m) => (
                  <option key={m.name} value={m.name}>{modelLabel(m)}</option>
                ))}
              </select>
            ) : selected ? modelLabel(selected) : "Select a model in the header"}
          </div>
        </div>
        <div>
          <div
            style={{
              fontSize: 11,
              color: "#64748b",
              marginBottom: 4,
              fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
              letterSpacing: "0.04em",
              textTransform: "uppercase",
              fontWeight: 650,
            }}
          >
            Status
          </div>
          <div
            style={{ fontSize: 13, color: "#475569", fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif" }}
            data-testid="cliff-read"
          >
            {running
              ? "Running…"
              : verdict.kind === "cliff"
                ? // A detected cliff ALWAYS reads as a cliff — when the collapse rung had no
                  // measured token count we say so, never falling through to a non-cliff message
                  // and never substituting a different rung's depth as if it were the cliff's.
                  cliff != null
                  ? `≈${Math.round(cliff / 1000) * 1000} context tokens`
                  : "Cliff detected — context-token depth not reported"
                : verdict.kind === "broken-baseline"
                  ? "Fails at the smallest tested context — broken baseline (a tool-call failure, not a context-length limit)"
                  : verdict.kind === "no-cliff" && maintainedTo > 0
                    ? `Accuracy maintained up to ≈${Math.round(maintainedTo / 1000) * 1000} tokens`
                    : points.length > 0
                      ? "Ran — context-token depth not reported"
                      : "Idle"}
          </div>
        </div>
      </div>

      {/* ── Method: native vs prompt-based tool-calling (the user's choice drives the probe) ── */}
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "14px 20px 0" }} data-testid="cliff-method">
        <span
          style={{
            fontSize: 12,
            color: "#64748b",
            fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
            whiteSpace: "nowrap",
            minWidth: 70,
          }}
        >
          Method
        </span>
        <div style={{ display: "flex", gap: 4 }}>
          {(["native_fc", "prompt_based"] as AgentPath[]).map((m) => {
            const isNative = m === "native_fc";
            const disabled = isNative && !nativeAvailable;
            const activeBtn = effectiveMethod === m;
            return (
              <button
                key={m}
                type="button"
                disabled={disabled}
                onClick={() => setMethod(m)}
                data-testid={`cliff-method-${isNative ? "native" : "prompt"}`}
                title={
                  disabled
                    ? "MLX has no native tool-calling API — use Prompt-based"
                    : isNative
                      ? "Probe native function-calling (structured tool_calls)"
                      : "Probe the prompt-based JSON-in-text tool proxy"
                }
                style={{
                  padding: "4px 12px",
                  borderRadius: 6,
                  fontSize: 12,
                  fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
                  cursor: disabled ? "not-allowed" : "pointer",
                  border: `1px solid ${activeBtn ? "#2563eb" : "#cbd5e1"}`,
                  background: activeBtn ? "#eff6ff" : "#ffffff",
                  color: disabled ? "#cbd5e1" : activeBtn ? "#1d4ed8" : "#475569",
                  fontWeight: activeBtn ? 600 : 400,
                }}
              >
                {isNative ? "Native FC" : "Prompt-based"}
              </button>
            );
          })}
        </div>
        {!nativeAvailable && (
          <span style={{ fontSize: 11, color: "#94a3b8", fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif" }}>
            MLX: prompt-based only
          </span>
        )}
      </div>

      {/* ── Sliders ── */}
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "1fr 1fr",
          padding: "14px 20px",
          gap: 16,
          borderBottom: "1px solid #f1f5f9",
        }}
      >
        {/* Max Tokens */}
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span
            style={{
              fontSize: 12,
              color: "#64748b",
              fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
              whiteSpace: "nowrap",
              minWidth: 70,
            }}
          >
            Max Tokens
          </span>
          <input
            type="range"
            min={4096}
            max={sliderMax}
            step={1024}
            value={maxTokens}
            onChange={(e) => setMaxTokens(Number(e.target.value))}
            data-testid="cliff-max-tokens"
            title={dims?.context_length ? `Capped at model context window (${dims.context_length})` : "Model context window unknown — fixed ceiling"}
            style={sliderStyle}
          />
          <span
            style={{
              fontSize: 12,
              color: "#334155",
              fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
              background: "#f1f5f9",
              border: "1px solid #e2e8f0",
              borderRadius: 6,
              padding: "2px 8px",
              minWidth: 48,
              textAlign: "center",
            }}
          >
            {maxTokens}
          </span>
        </div>

        {/* Test Steps */}
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span
            style={{
              fontSize: 12,
              color: "#64748b",
              fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
              whiteSpace: "nowrap",
              minWidth: 66,
            }}
          >
            Test Steps
          </span>
          <input
            type="range"
            min={2}
            max={10}
            step={1}
            value={testSteps}
            onChange={(e) => setTestSteps(Number(e.target.value))}
            data-testid="cliff-test-steps"
            style={sliderStyle}
          />
          <span
            style={{
              fontSize: 12,
              color: "#334155",
              fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
              background: "#f1f5f9",
              border: "1px solid #e2e8f0",
              borderRadius: 6,
              padding: "2px 8px",
              minWidth: 32,
              textAlign: "center",
            }}
          >
            {testSteps}
          </span>
        </div>

        {/* Padding source — which license-clean synthetic preset fills the context.
            The backend cycles it, char-boundary-safe, to each verified token depth. */}
        <label
          style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 14, fontSize: 12, color: "#475569", fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif" }}
          title="The filler the probe pads the context with before the real instruction. Greedy (temp 0) is always on — a diagnostic must reproduce."
        >
          Padding
          <select
            value={preset}
            onChange={(e) => setPreset(e.target.value as CliffPreset)}
            data-testid="cliff-source-select"
            style={{ fontSize: 12, color: "#334155", background: "#f1f5f9", border: "1px solid #e2e8f0", borderRadius: 6, padding: "3px 8px", outline: "none", cursor: "pointer", fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif" }}
          >
            <option value="corporate_policy">Corporate Policy (prose)</option>
            <option value="system_logs">System Logs (structured)</option>
            <option value="financial_ledger">Financial Ledger (tabular)</option>
          </select>
        </label>
      </div>

      {/* ── Progress + Execute / Stop ── */}
      <div style={{ padding: "14px 20px" }}>
        {/* Pre-flight memory advisory (Ollama) — warns before the click; the backend still
            hard-stops a run that truly won't fit. Advisory only, so it never disables Execute. */}
        {fitWarning && !running && (
          <div
            data-testid="cliff-fit-warning"
            style={{
              marginBottom: 12,
              fontSize: 11,
              fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
              color: "#92400e",
              background: "#fffbeb",
              border: "1px solid #fde68a",
              borderRadius: 6,
              padding: "6px 10px",
            }}
          >
            ⚠ {fitWarning}
          </div>
        )}
        {running && (
          <div data-testid="cliff-progress" style={{ marginBottom: 12 }}>
            {/* Headline: which rung + the depth it's padding to, plus an overall % so the
                user sees the whole-run position at a glance. The deep rungs are the slow
                ones, so the rung count alone can sit still for minutes — the sub-line and
                ETA below are what prove the run is alive. */}
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", gap: 12, fontSize: 11, color: "#475569", fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif", marginBottom: 3 }}>
              <span>
                Probing {runningModel ?? model} — rung {step ? step.rung : progress.done}/{step ? step.total_rungs : progress.total}
                {step && step.target_tokens > 0
                  ? ` · padding to ${(step.target_tokens / 1000).toFixed(1)}k tokens`
                  : step && step.rung === 1
                    ? " · unpadded baseline"
                    : lastDepth != null
                      ? ` · ~${(lastDepth / 1000).toFixed(1)}k tokens`
                      : ""}
              </span>
              <span style={{ fontVariantNumeric: "tabular-nums", color: "#334155", fontWeight: 600 }}>{Math.round(frac * 100)}%</span>
            </div>
            {/* Sub-line: the per-task ticker (position p/3 · task t/M) + elapsed/ETA. This is
                the "it's not stuck" signal — it advances every time a single generation
                returns, even while a single rung grinds for minutes. */}
            <div style={{ fontSize: 10.5, color: "#94a3b8", fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif", marginBottom: 6 }}>
              {step
                ? `position ${step.position}/${step.total_positions} · task ${step.task}/${step.total_tasks} · ${formatDuration(elapsedS)} elapsed${etaS != null ? ` · ~${formatDuration(etaS)} left` : ""}`
                : "starting…"}{" "}
              · keep this tab open or switch away — the run continues.
            </div>
            <div style={{ height: 5, background: "#f1f5f9", borderRadius: 3 }}>
              <div
                style={{
                  height: 5,
                  width: `${Math.round(frac * 100)}%`,
                  background: "#2563eb",
                  borderRadius: 3,
                  transition: "width 200ms ease",
                }}
              />
            </div>
          </div>
        )}
        <button
          type="button"
          disabled={running ? false : !selected || tasks.length === 0}
          onClick={running ? handleStop : handleRun}
          data-testid="cliff-run"
          style={{
            width: "100%",
            padding: "12px 0",
            borderRadius: 10,
            border: running ? "1px solid #fca5a5" : "none",
            background: running ? "#fee2e2" : !selected || tasks.length === 0 ? "#f1f5f9" : "#0f172a",
            color: running ? "#991b1b" : !selected || tasks.length === 0 ? "#94a3b8" : "#ffffff",
            fontSize: 14,
            fontWeight: 600,
            fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
            letterSpacing: "0.02em",
            cursor: running || (selected && tasks.length > 0) ? "pointer" : "not-allowed",
            transition: "all 0.2s",
          }}
        >
          {running ? "■ Stop Probe" : "Execute Probe"}
        </button>
      </div>
    </div>
  );
}

// ── Helpers ────────────────────────────────────────────────────────────────────

/// Compact, human "Nm Ss" / "Ns" for the elapsed + ETA readout. Rounds to whole seconds
/// — sub-second precision is noise on a multi-minute probe.
function formatDuration(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  const rem = s % 60;
  return rem === 0 ? `${m}m` : `${m}m ${rem}s`;
}

// ── Shared styles ──────────────────────────────────────────────────────────────

const tdStyle: React.CSSProperties = {
  padding: "8px 12px",
  fontSize: 13,
  color: "#475569",
  fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
  borderBottom: "1px solid #f1f5f9",
};

const passChipStyle: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: 5,
  background: "rgba(16,185,129,0.06)",
  border: "1px solid rgba(16,185,129,0.2)",
  color: "#059669",
  borderRadius: 6,
  padding: "2px 8px",
  fontSize: 12,
  fontWeight: 650,
  fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
};

const failChipStyle: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: 5,
  background: "rgba(220,38,38,0.05)",
  border: "1px solid rgba(220,38,38,0.2)",
  color: "#dc2626",
  borderRadius: 6,
  padding: "2px 8px",
  fontSize: 12,
  fontWeight: 650,
  fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
};

const sliderStyle: React.CSSProperties = {
  flex: 1,
  accentColor: "#2563eb",
  cursor: "pointer",
  height: 4,
};

// ── Per-step "View trace" expansion ──────────────────────────────────────────────

const traceBtnStyle: React.CSSProperties = {
  background: "#ffffff",
  border: "1px solid #cbd5e1",
  color: "#334155",
  borderRadius: 6,
  padding: "3px 10px",
  fontSize: 12,
  fontWeight: 600,
  fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
  cursor: "pointer",
};

const traceCardStyle: React.CSSProperties = {
  border: "1px solid #e2e8f0",
  background: "#ffffff",
  borderRadius: 8,
  padding: "10px 12px",
  marginTop: 8,
};

const traceTaskStyle: React.CSSProperties = {
  fontSize: 12,
  fontWeight: 650,
  color: "#475569",
  fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
  marginBottom: 6,
};

const traceLabelStyle: React.CSSProperties = {
  fontSize: 11,
  color: "#64748b",
  fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
  letterSpacing: "0.03em",
  textTransform: "uppercase",
  fontWeight: 650,
  marginBottom: 4,
};

const traceSubLabelStyle: React.CSSProperties = {
  fontSize: 10,
  color: "#94a3b8",
  fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
  letterSpacing: "0.03em",
  textTransform: "uppercase",
  fontWeight: 600,
  marginBottom: 3,
};

const tracePreStyle: React.CSSProperties = {
  margin: 0,
  whiteSpace: "pre-wrap",
  wordBreak: "break-word",
  fontSize: 12,
  lineHeight: 1.45,
  color: "#334155",
  fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
  background: "#f8fafc",
  border: "1px solid #f1f5f9",
  borderRadius: 6,
  padding: "6px 8px",
  maxHeight: 220,
  overflow: "auto",
};

