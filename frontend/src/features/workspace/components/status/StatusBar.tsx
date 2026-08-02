import { useWorkspaceStore } from "../../state/workspaceStore";
import { useBackendStore } from "../../../../shared/state/backendStore";
import { formatMetrics } from "../../format";
import { backendStatus } from "./backendStatus";

type Props = {
  model: string | null;
  onModelClick?: () => void;
};

export function StatusBar({ model, onModelClick }: Props) {
  const metrics = useWorkspaceStore((s) => s.lastRunMetrics);
  const activeBackend = useBackendStore((s) => s.selectedBackend);
  const llamaHealthy = useBackendStore((s) => s.llamaHealthy);
  const vllmHealthy = useBackendStore((s) => s.vllmHealthy);

  // The status reflects the active backend: llama.cpp tracks its server's run
  // state, the remote backends their reachability. Each names the loaded model.
  const { running, label: healthLabel, aria: healthAria } = backendStatus(
    activeBackend,
    llamaHealthy,
    vllmHealthy,
    model,
  );
  const dotClass = running ? "bg-green-500" : "bg-red-500";

  return (
    <footer
      data-testid="status-bar"
      className="fixed bottom-0 left-0 right-0 h-10 px-3 flex items-center justify-between text-xs font-mono border-t bg-surface"
    >
      <button
        type="button"
        onClick={onModelClick}
        className="hover:underline disabled:no-underline"
        disabled={!onModelClick}
      >
        {model ?? "no model"}
      </button>
      <span className="flex items-center gap-1.5" aria-label={healthAria}>
        <span className={`inline-block w-2 h-2 rounded-full ${dotClass}`} />
        {healthLabel}
      </span>
      <span data-testid="status-bar-metrics">
        {metrics ? formatMetrics(metrics) : "no run yet"}
      </span>
    </footer>
  );
}
