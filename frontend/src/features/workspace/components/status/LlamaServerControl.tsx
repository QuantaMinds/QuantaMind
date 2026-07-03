import { useEffect, useRef, useState } from "react";
import { useBackendStore } from "../../../../shared/state/backendStore";
import { useSelectedModelStore } from "../../../../shared/state/selectedModelStore";
import { useParamsStore } from "../../../../shared/state/paramsStore";
import { useStartLlamaServer } from "../../hooks/useStartLlamaServer";
import { useStopLlamaServer } from "../../hooks/useStopLlamaServer";
import { PlayStopButton } from "../../../../shared/ui/PlayStopButton";

/// A compact status chip for the llama-server launch note/failure. The backend's
/// message is long prose (a hardware-constraint note, or a start error); rendered inline
/// it crushed the header row, so it collapses to a small ⚠ chip and the full text lives
/// in a hover popover. A FRESH message auto-opens the popover once (keyed on the message
/// content, so a second distinct message re-greets), then a timer collapses it to
/// hover-only. The full text stays mounted in the popover (visibility-toggled) so it's
/// always in the DOM under `testId` — the header stays a single clean row either way.
function LlamaStartBadge({ kind, message, testId }: { kind: "error" | "notice"; message: string; testId: string }) {
  const [open, setOpen] = useState(true);
  const hovering = useRef(false);
  useEffect(() => {
    setOpen(true);
    const id = setTimeout(() => {
      if (!hovering.current) setOpen(false);
    }, 6000);
    return () => clearTimeout(id);
  }, [message]);

  const isError = kind === "error";
  const chip = isError
    ? "border-red-200 bg-red-50 text-red-600"
    : "border-amber-200 bg-amber-50 text-amber-600";
  return (
    <span
      data-testid={testId}
      className="relative inline-flex"
      onMouseEnter={() => {
        hovering.current = true;
        setOpen(true);
      }}
      onMouseLeave={() => {
        hovering.current = false;
        setOpen(false);
      }}
    >
      <span className={`inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[10px] font-medium cursor-default ${chip}`}>
        <span aria-hidden>⚠</span> {isError ? "Start failed" : "Running safely"}
      </span>
      <span
        role="tooltip"
        data-testid={`${testId}-popup`}
        className={`absolute left-0 top-6 z-30 w-72 rounded-lg border border-white/15 bg-slate-800 p-3 text-[11px] leading-relaxed text-slate-100 shadow-xl transition-opacity ${
          open ? "opacity-100" : "pointer-events-none opacity-0"
        }`}
      >
        {message}
      </span>
    </span>
  );
}

/// Header play/stop for the llama-server sidecar. Play launches the server on the
/// global llama.cpp model's GGUF (one model at a time); stop kills it.
export function LlamaServerControl() {
  const healthy = useBackendStore((s) => s.llamaHealthy);
  const model = useSelectedModelStore((s) =>
    s.selectedModels.find((m) => m.backend === "llama_cpp") ?? null,
  );
  const { start, status: startStatus, error: startError, notice: startNotice } = useStartLlamaServer();
  const { stop, status: stopStatus } = useStopLlamaServer();
  const path = model?.path;
  // The user's "Context window" param drives the launch `-c` for llama.cpp
  // (its context is fixed at spawn); changing it relaunches the server.
  const numCtx = useParamsStore((s) => s.globalParams.num_ctx);

  return (
    // Horizontal so the Play/Stop button and the status chip share the header row line —
    // stacking them vertically made this control taller than the dropdowns and `items-center`
    // shoved the button up out of the row.
    <div className="flex items-center gap-1.5">
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
      {/* A long backend note/error would crush the header inline — collapse it to a compact
          chip whose full text opens in a hover popover (error wins if both are present). */}
      {startError ? (
        <LlamaStartBadge kind="error" message={startError} testId="llama-start-error" />
      ) : startNotice ? (
        <LlamaStartBadge kind="notice" message={startNotice} testId="llama-start-notice" />
      ) : null}
    </div>
  );
}
