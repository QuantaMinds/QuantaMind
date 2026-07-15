import { useCallback, useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  AgenticStepPayloadSchema,
  BatchCompletePayloadSchema,
  BatchProgressSchema,
  EVENT_AGENTIC_STEP,
  EVENT_BATCH_COMPLETE,
  EVENT_BATCH_PROGRESS,
  runBatchEval,
  stopBatchEval,
  type ThinkPreset,
} from "../../../shared/ipc/eval/batch";
import { formatIpcError } from "../../../shared/ipc/core/error";
import { healthFor, credentialFor } from "../../../shared/ipc/core/client";
import { remoteCredentialMessage } from "../remoteCredential";
import type { ModelTarget } from "../../../shared/ipc/eval/matrix";
import type { ToolTask } from "../../../shared/ipc/eval/registry";
import type { Tier } from "../../../shared/ipc/eval/readiness";
import { useInstalledModelsStore } from "../../models/state/installedModelsStore";
import { useBatchStore } from "../state/batchStore";
import { useParamsStore } from "../../../shared/state/paramsStore";
import { useModelSettingsStore } from "../../models/state/modelSettingsStore";

const modelSig = (list: { name: string }[]) =>
  list
    .map((m) => m.name)
    .sort()
    .join("|");

/// Drives one batch run over a single Tauri event stream. Subscribes ONCE to
/// `batch-progress` / `agentic-step` / `batch-complete`, routes each into the
/// rAF-buffered store, and flushes that store if the installed-model set changes
/// (so a stale trajectory never renders against the wrong model row).
export function useBatchRun() {
  useEffect(() => {
    const unlisten: UnlistenFn[] = [];
    let cancelled = false;
    void (async () => {
      const subs = await Promise.all([
        listen(EVENT_BATCH_PROGRESS, (e) => {
          const r = BatchProgressSchema.safeParse(e.payload);
          if (r.success) useBatchStore.getState().ingestProgress(r.data);
          else console.error("IPC payload drift (batch-progress):", r.error.issues, e.payload);
        }),
        listen(EVENT_AGENTIC_STEP, (e) => {
          const r = AgenticStepPayloadSchema.safeParse(e.payload);
          if (r.success) useBatchStore.getState().ingestStep(r.data);
          else console.error("IPC payload drift (agentic-step):", r.error.issues, e.payload);
        }),
        listen(EVENT_BATCH_COMPLETE, (e) => {
          const r = BatchCompletePayloadSchema.safeParse(e.payload);
          if (r.success) useBatchStore.getState().complete(r.data.report, r.data.final);
          else console.error("IPC payload drift (batch-complete):", r.error.issues, e.payload);
        }),
      ]);
      if (cancelled) {
        subs.forEach((u) => u());
        return;
      }
      unlisten.push(...subs);
    })();
    return () => {
      cancelled = true;
      unlisten.forEach((u) => u());
    };
  }, []);

  // Cache invalidation: when the installed-model set actually changes (a swap /
  // download), flush volatile traces — but never wipe a run in progress.
  useEffect(() => {
    let prev = modelSig(useInstalledModelsStore.getState().list);
    return useInstalledModelsStore.subscribe((s) => {
      const next = modelSig(s.list);
      if (next !== prev) {
        prev = next;
        if (!useBatchStore.getState().running) useBatchStore.getState().reset();
      }
    });
  }, []);

  const run = useCallback(
    async (
      collectionId: string,
      targets: ModelTarget[],
      tasks: ToolTask[],
      k?: number,
      maxSteps?: number,
      runNativeFc?: boolean,
      tier?: Tier,
      decoyTools?: number,
      runPromptBased?: boolean,
      thinkPreset?: ThinkPreset,
    ) => {
      // Pre-flight: actively probe EVERY backend this run uses (a run can mix
      // backends), so a down server fails fast with a clear message instead of
      // hanging mid-run. Active probe, not the cached store value, so it's correct
      // even before the 5s poll first ticks.
      const backends = Array.from(new Set(targets.map((t) => t.backend)));
      for (const backend of backends) {
        // Remote (vLLM/SGLang): resolve+validate the CREDENTIAL up front, so a bad/expired key
        // fails fast here with the right message (a 401 ≠ "unreachable"), not a mid-run 401.
        if (backend === "vllm" || backend === "sglang") {
          let msg: string | null;
          try {
            msg = remoteCredentialMessage(backend, await credentialFor(backend));
          } catch {
            const label = backend === "sglang" ? "SGLang" : "vLLM";
            msg = `Couldn't validate the ${label} endpoint — check it in Settings, then re-run.`;
          }
          if (msg) {
            useBatchStore.getState().setError(msg);
            return;
          }
          continue;
        }
        // Local backends: a reachability probe is enough (no credential).
        let available = false;
        try {
          available = (await healthFor(backend)).available;
        } catch {
          available = false;
        }
        if (!available) {
          const label = backend === "llama_cpp" ? "llama.cpp" : backend === "mlx" ? "MLX" : "Ollama";
          useBatchStore
            .getState()
            .setError(`${label} server isn't reachable — start it from the Workspace status bar, then re-run.`);
          return;
        }
      }

      useBatchStore.getState().startRun();
      try {
        const { globalParams, keepLoaded } = useParamsStore.getState();
        // Stamp each target with its persisted thinking flag, so the backend raises the
        // token budget + strips <think> for reasoning models. Read fresh at dispatch.
        const isThinkingFor = useModelSettingsStore.getState().isThinkingFor;
        const thinkingTargets = targets.map((t) => ({ ...t, is_thinking: isThinkingFor(t.model) }));
        // Keep loaded → resident; off → omit so the backend default applies.
        await runBatchEval(
          collectionId,
          thinkingTargets,
          tasks,
          k,
          maxSteps,
          globalParams,
          keepLoaded ? -1 : undefined,
          runNativeFc,
          tier,
          decoyTools,
          runPromptBased,
          thinkPreset,
        );
      } catch (e) {
        useBatchStore.getState().setError(formatIpcError(e));
      }
    },
    [],
  );

  // Bring-Your-Own diagnostic: same event stream + store gate as a Built-In run, but a
  // single live-server task with no answer key (schema-valid + attribution, no pass^k).
  const runByo = useCallback(
    async (
      serverId: string,
      taskName: string,
      instruction: string,
      target: ModelTarget,
    ) => {
      // Local-backend reachability probe (BYO is local-only — the server is a subprocess).
      let available = false;
      try {
        available = (await healthFor(target.backend)).available;
      } catch {
        available = false;
      }
      if (!available) {
        const label =
          target.backend === "llama_cpp" ? "llama.cpp" : target.backend === "mlx" ? "MLX" : "Ollama";
        useBatchStore
          .getState()
          .setError(`${label} server isn't reachable — start it from the Workspace status bar, then re-run.`);
        return;
      }
      useBatchStore.getState().startRun();
      try {
        const { runMcpByoBatch } = await import("../../../shared/ipc/mcp/run");
        await runMcpByoBatch(target.model, target.backend, serverId, taskName, instruction);
      } catch (e) {
        useBatchStore.getState().setError(formatIpcError(e));
      }
    },
    [],
  );

  const stop = useCallback(async () => {
    // Flip BEFORE the await so the button shows "Stopping…" the instant it's clicked,
    // not after the IPC round-trip (and the in-flight turn/run behind it) resolves.
    useBatchStore.getState().beginStop();
    await stopBatchEval();
  }, []);

  return { run, runByo, stop };
}
