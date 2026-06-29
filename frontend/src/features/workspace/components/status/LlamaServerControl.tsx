import { useBackendStore } from "../../../../shared/state/backendStore";
import { useSelectedModelStore } from "../../../../shared/state/selectedModelStore";
import { useParamsStore } from "../../../../shared/state/paramsStore";
import { useStartLlamaServer } from "../../hooks/useStartLlamaServer";
import { useStopLlamaServer } from "../../hooks/useStopLlamaServer";
import { PlayStopButton } from "../../../../shared/ui/PlayStopButton";

/// Header play/stop for the llama-server sidecar. Play launches the server on the
/// global llama.cpp model's GGUF (one model at a time); stop kills it.
export function LlamaServerControl() {
  const healthy = useBackendStore((s) => s.llamaHealthy);
  const model = useSelectedModelStore((s) =>
    s.selectedModels.find((m) => m.backend === "llama_cpp") ?? null,
  );
  const { start, status: startStatus, error: startError } = useStartLlamaServer();
  const { stop, status: stopStatus } = useStopLlamaServer();
  const path = model?.path;
  // The user's "Context window" param drives the launch `-c` for llama.cpp
  // (its context is fixed at spawn); changing it relaunches the server.
  const numCtx = useParamsStore((s) => s.globalParams.num_ctx);

  return (
    <div className="space-y-0.5">
      <PlayStopButton
        running={!!healthy}
        busy={startStatus === "starting" || stopStatus === "stopping"}
        disabled={!path}
        onPlay={() => path && void start(path, numCtx)}
        onStop={() => void stop()}
        title={path ? "Start llama-server on the selected model" : "Select a llama.cpp model first"}
        label="llama.cpp"
        playTestId="llama-start"
        stopTestId="llama-stop"
      />
      {startError && (
        <p data-testid="llama-start-error" className="px-2 text-[10px] text-red-600">
          {startError}
        </p>
      )}
    </div>
  );
}
