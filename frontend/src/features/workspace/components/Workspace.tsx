import { PromptEditor } from "./prompt/PromptEditor";
import { StatusBar } from "./status/StatusBar";
import { ModelSelectBar } from "./model-select/ModelSelectBar";
import { SingleRun } from "./run/SingleRun";
import { PromptTemplatePicker } from "./PromptTemplatePicker";
import { BackendSetupGuide } from "./BackendSetupGuide";
import { useWorkspacesStore } from "../../workspaces/state/workspaceStore";
import { useSelectedModelStore } from "../../../shared/state/selectedModelStore";
import { useBackendStore } from "../../../shared/state/backendStore";

/// The run surface, driven by the global header selection. One model → a single
/// streaming run; results land
/// on the Analysis tab. When no LLM backend is running, the setup guide takes
/// over until a server comes up (StatusBar stays mounted so health keeps polling).
export function Workspace() {
  const current = useWorkspacesStore((s) => s.current);
  const patch = useWorkspacesStore((s) => s.patch);
  const selectedModels = useSelectedModelStore((s) => s.selectedModels);
  const noLlmRunning = useBackendStore(
    (s) => s.llamaHealthy !== true && s.vllmHealthy !== true,
  );
  const model = selectedModels[0]?.name ?? null;

  return (
    <div className="space-y-3">
      {noLlmRunning ? (
        <BackendSetupGuide />
      ) : (
        <>
          <ModelSelectBar />
          {!current ? (
        <p data-testid="workspace-empty" className="text-sm text-gray-500 px-2 py-8 text-center">
          Select a prompt from the Files panel, or click <strong>+ New</strong> to create one.
        </p>
      ) : (
        <>
          <PromptEditor
            value={current.system}
            onChange={(v) => patch({ system: v })}
            label="System prompt (optional)"
            testId="system-prompt-editor"
            height="120px"
          />
          <div className="flex items-center justify-between">
            <span className="text-xs text-gray-600">User prompt</span>
            <PromptTemplatePicker onInsert={(body) => patch({ user: body })} />
          </div>
          <PromptEditor
            value={current.user}
            onChange={(v) => patch({ user: v })}
            testId="user-prompt-editor"
          />
          <SingleRun model={model} />
            </>
          )}
        </>
      )}
      <StatusBar model={model} onModelClick={() => undefined} />
    </div>
  );
}
