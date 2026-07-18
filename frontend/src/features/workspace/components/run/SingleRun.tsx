import { useEffect } from "react";
import { RunControls } from "./RunControls";
import { useStreamingRun } from "../../hooks/useStreamingRun";
import { useWorkspaceHotkeys } from "../../hooks/useWorkspaceHotkeys";
import { useBackendStore } from "../../../../shared/state/backendStore";
import { useWorkspacesStore } from "../../../workspaces/state/workspaceStore";
import { useNavStore } from "../../../../shared/state/navStore";
import { useCompareStore } from "../../../compare/state/compareStore";
import { useParamsStore } from "../../../../shared/state/paramsStore";
import { backendRunHint } from "../../state/runHint";
import { workspacePointerCommand } from "../../../../shared/cli/qmCommand";

/// Single-model run trigger: run_prompt streaming with per-prompt params and
/// history. The response is shown on the Analysis tab — this mirrors the live
/// run into compareStore.rows and navigates there on Run.
export function SingleRun({ model }: { model: string | null }) {
  const current = useWorkspacesStore((s) => s.current);
  const currentPath = useWorkspacesStore((s) => s.currentPath);
  const save = useWorkspacesStore((s) => s.save);
  const saveDraftAuto = useWorkspacesStore((s) => s.saveDraftAuto);
  const ollamaHealthy = useBackendStore((s) => s.ollamaHealthy);
  const llamaHealthy = useBackendStore((s) => s.llamaHealthy);
  const mlxHealthy = useBackendStore((s) => s.mlxHealthy);
  const vllmHealthy = useBackendStore((s) => s.vllmHealthy);
  const sglangHealthy = useBackendStore((s) => s.sglangHealthy);
  const activeBackend = useBackendStore((s) => s.selectedBackend);
  const setSingleRun = useCompareStore((s) => s.setSingleRun);
  const active = useNavStore((s) => s.topView) === "workspace";
  const { output, status, error, metrics, start, cancel } = useStreamingRun();

  useEffect(() => {
    if (status === "done") void saveDraftAuto();
  }, [status, saveDraftAuto]);

  // Mirror the live run into the rows the Analysis tab renders.
  useEffect(() => {
    if (status === "idle" || !model) return;
    setSingleRun({
      model, modelId: null, status, output, metrics,
      error: error ? { kind: "error", message: error } : null,
      startedAt: null, endedAt: null,
    });
  }, [status, output, metrics, error, model, setSingleRun]);

  const prompt = current?.user ?? "";
  const system = current?.system ?? "";
  // A backend is coupled to the model's weight format — no fallback. If the
  // active backend isn't healthy, Run is blocked with a "start it" hint.
  const blockedHint = backendRunHint(activeBackend, {
    ollama: ollamaHealthy,
    llama: llamaHealthy,
    mlx: mlxHealthy,
    vllm: vllmHealthy,
    sglang: sglangHealthy,
  });
  const canRun = !!model && prompt.trim().length > 0 && !blockedHint;
  const runNow = () => {
    if (!model) return;
    useNavStore.getState().setTopView("compare");
    void start(model, prompt, system, useParamsStore.getState().globalParams, currentPath, current?.name);
  };
  useWorkspaceHotkeys({
    active, canRun, running: status === "running", hasPrompt: !!current,
    onRun: runNow, onStop: cancel, onSave: () => void save(),
  });

  return (
    <div className="space-y-1">
      {!model && (
        <p className="text-xs text-amber-700" data-testid="no-model-hint">
          Pick a model in the header to run.
        </p>
      )}
      <RunControls
        status={status}
        canRun={canRun}
        blockedHint={blockedHint}
        onRun={runNow}
        onCancel={cancel}
      />
      {/* A single interactive prompt has no headless `qm` equivalent — point to the
          eval path instead of fabricating a command. */}
      <p className="text-[10px] text-gray-400 leading-snug" data-testid="workspace-cli-hint">
        No CLI equivalent for a single prompt. To evaluate this model headless, use the Tests tab:{" "}
        <code className="bg-gray-50 border rounded px-1 py-0.5 font-mono text-gray-500">
          {workspacePointerCommand(model, activeBackend)}
        </code>
      </p>
    </div>
  );
}
