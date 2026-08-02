import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  CancelledPayloadSchema,
  DonePayloadSchema,
  EVENT_CANCELLED,
  EVENT_DONE,
  EVENT_TOKEN,
  TokenPayloadSchema,
  type CancelledPayload,
  type DonePayload,
} from "../../../shared/ipc/events/events";
import { withTimeout } from "../../../shared/ipc/core/timeout";
import { formatIpcError } from "../../../shared/ipc/core/error";
import { useWorkspaceStore } from "../state/workspaceStore";
import { useBackendStore } from "../../../shared/state/backendStore";
import { useParamsStore } from "../../../shared/state/paramsStore";
import type { InferenceParams } from "../../../shared/ipc/workspace/prompts";
import { recordRun, type RunContext } from "../../history/recordRun";
import { useLeakStore } from "../../inspector/state/leakStore";

const hasParam = (p?: InferenceParams) =>
  !!p && Object.values(p).some((v) => v !== undefined && v !== null);

export type RunStatus = "idle" | "running" | "done" | "cancelled" | "error";

// No timeout on run_prompt: model loads + long generations are legitimate.
// User cancels via the Stop button (stop_prompt), which is bounded below.
export const STOP_PROMPT_TIMEOUT_MS = 5_000;

export function useStreamingRun() {
  const [output, setOutput] = useState("");
  const [status, setStatus] = useState<RunStatus>("idle");
  const [error, setError] = useState<string | null>(null);
  const [metrics, setMetrics] = useState<DonePayload | null>(null);
  const [cancelledInfo, setCancelledInfo] = useState<CancelledPayload | null>(null);
  const outputRef = useRef("");
  const ctxRef = useRef<RunContext | null>(null);
  // True only between this hook's own start() and the run's terminal event. The
  // run_prompt event stream is global and other hooks may also listen, so we
  // react only to the run we initiated — no stray history,
  // leak samples, or compareStore writes for someone else's run.
  const initiatedRef = useRef(false);

  useEffect(() => {
    let cancelled = false;
    const unsubs: Array<() => void> = [];
    const fail = (label: string, issues: unknown) => {
      initiatedRef.current = false;
      console.error(`invalid ${label} payload`, issues);
      setError("invalid backend payload"); setStatus("error");
    };
    (async () => {
      const ut = await listen<unknown>(EVENT_TOKEN, (e) => {
        if (!initiatedRef.current) return;
        const p = TokenPayloadSchema.safeParse(e.payload);
        if (!p.success) return fail("prompt-token", p.error.issues);
        outputRef.current += p.data.text;
        setOutput((prev) => prev + p.data.text);
      });
      if (cancelled) { ut(); return; }
      unsubs.push(ut);
      const ud = await listen<unknown>(EVENT_DONE, (e) => {
        if (!initiatedRef.current) return;
        const p = DonePayloadSchema.safeParse(e.payload);
        if (!p.success) return fail("prompt-done", p.error.issues);
        initiatedRef.current = false;
        setMetrics(p.data); setStatus("done");
        useWorkspaceStore.getState().setLastRunMetrics(p.data);
        void recordRun(ctxRef.current, outputRef.current, {
          token_count: p.data.token_count,
          ttft_ms: p.data.ttft_ms,
          tokens_per_sec: p.data.tokens_per_sec,
          load_ms: p.data.stats?.load_ms,
        });
        void useLeakStore.getState().sample(ctxRef.current?.model ?? "");
      });
      if (cancelled) { ud(); return; }
      unsubs.push(ud);
      const uc = await listen<unknown>(EVENT_CANCELLED, (e) => {
        if (!initiatedRef.current) return;
        const p = CancelledPayloadSchema.safeParse(e.payload);
        if (!p.success) return fail("prompt-cancelled", p.error.issues);
        initiatedRef.current = false;
        setCancelledInfo(p.data); setStatus("cancelled");
      });
      if (cancelled) { uc(); return; }
      unsubs.push(uc);
    })();
    return () => { cancelled = true; unsubs.forEach((u) => u()); };
  }, []);

  const start = useCallback(
    async (model: string, prompt: string, system?: string, params?: InferenceParams, promptPath?: string | null, name?: string) => {
      setOutput(""); setMetrics(null); setCancelledInfo(null); setError(null);
      outputRef.current = ""; ctxRef.current = { name, model, prompt, system, params, promptPath };
      initiatedRef.current = true;
      setStatus("running");
      try {
        const args: Record<string, unknown> = { model, prompt };
        const trimmed = system?.trim();
        if (trimmed) args.system = trimmed;
        if (hasParam(params)) args.params = params;
        args.backend = useBackendStore.getState().selectedBackend;
        // Keep loaded → Ollama keep_alive=-1 (resident). Off → omit it so Ollama's
        // default idle-unload applies: the model lingers (and stays inspectable in
        // the Inspector) for a few minutes, then frees memory. Sending 0 would
        // unload instantly and leave nothing for the Inspector to read.
        if (useParamsStore.getState().keepLoaded) args.keepAlive = -1;
        await invoke("run_prompt", args);
      } catch (e) {
        initiatedRef.current = false;
        setError(formatIpcError(e)); setStatus("error");
      }
    },
    [],
  );

  const cancel = useCallback(async () => {
    try { await withTimeout(invoke("stop_prompt"), STOP_PROMPT_TIMEOUT_MS, "stop_prompt"); }
    catch { /* best-effort: backend may have already finished */ }
  }, []);

  return { output, status, error, metrics, cancelledInfo, start, cancel };
}
