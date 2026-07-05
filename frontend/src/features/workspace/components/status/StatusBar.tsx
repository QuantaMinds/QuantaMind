import { useEffect, useState } from "react";
import { checkOllamaHealth } from "../../../../shared/ipc/core/client";
import type { HealthStatus } from "../../../../shared/ipc/core/types";
import { useWorkspaceStore } from "../../state/workspaceStore";
import { useBackendStore } from "../../../../shared/state/backendStore";
import { formatMetrics } from "../../format";
import { backendStatus } from "./backendStatus";

const POLL_MS = 5000;

type Props = {
  model: string | null;
  onModelClick?: () => void;
};

export function StatusBar({ model, onModelClick }: Props) {
  const metrics = useWorkspaceStore((s) => s.lastRunMetrics);
  const setOllamaHealthy = useBackendStore((s) => s.setOllamaHealthy);
  const activeBackend = useBackendStore((s) => s.selectedBackend);
  const llamaHealthy = useBackendStore((s) => s.llamaHealthy);
  const mlxHealthy = useBackendStore((s) => s.mlxHealthy);
  const vllmHealthy = useBackendStore((s) => s.vllmHealthy);
  const sglangHealthy = useBackendStore((s) => s.sglangHealthy);
  const [health, setHealth] = useState<HealthStatus | null>(null);

  useEffect(() => {
    let cancelled = false;
    const tick = async () => {
      try {
        const h = await checkOllamaHealth();
        if (!cancelled) { setHealth(h); setOllamaHealthy(h.available); }
      } catch {
        if (!cancelled) { setHealth({ available: false, version: null }); setOllamaHealthy(false); }
      }
    };
    tick();
    const id = setInterval(tick, POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [setOllamaHealthy]);

  // The status reflects the active backend, not always Ollama. llama.cpp and
  // MLX track their server's run state and name the loaded model.
  const { running, label: healthLabel, aria: healthAria } = backendStatus(
    activeBackend,
    health,
    llamaHealthy,
    mlxHealthy,
    vllmHealthy,
    sglangHealthy,
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
