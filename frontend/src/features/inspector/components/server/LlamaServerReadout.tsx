import { useEffect, useState } from "react";
import { llamaServerInfo, type LlamaServerInfo } from "../../../../shared/ipc/models/llama_start";
import { useBackendStore } from "../../../../shared/state/backendStore";
import { formatBytes } from "../../../../shared/format/bytes";

/// A one-time **spawn** readout for llama.cpp, shown only when it's the active
/// backend. Unlike a per-request loader (which surfaces load as a phase),
/// llama.cpp loads the model once at server spawn and keeps it resident — so this
/// is a server-startup fact, deliberately separate from the per-request TTFT phase
/// bar, not faked into it. Nothing renders when no server is up or the readout is
/// absent (never a fabricated number).
export function LlamaServerReadout() {
  const selectedBackend = useBackendStore((s) => s.selectedBackend);
  const llamaHealthy = useBackendStore((s) => s.llamaHealthy);
  const [info, setInfo] = useState<LlamaServerInfo>(null);

  useEffect(() => {
    if (selectedBackend !== "llama_cpp") {
      setInfo(null);
      return;
    }
    let cancelled = false;
    llamaServerInfo()
      .then((r) => !cancelled && setInfo(r))
      .catch(() => !cancelled && setInfo(null));
    return () => {
      cancelled = true;
    };
  }, [selectedBackend, llamaHealthy]);

  if (selectedBackend !== "llama_cpp" || !info) return null;

  const size = info.model_bytes != null ? formatBytes(info.model_bytes) : null;
  const loadS = (info.load_ms / 1000).toFixed(1);
  return (
    <div
      data-testid="llama-spawn-readout"
      className="flex items-center gap-3 text-sm text-slate-700 border border-slate-200 rounded-lg px-4 py-3 bg-white shadow-sm"
      title="llama.cpp loads the model once at server start and keeps it resident — this is a one-time startup cost, not part of each request's TTFT."
    >
      <div className="flex items-center gap-2 font-medium text-slate-900">
        <span className="relative flex h-2.5 w-2.5">
          <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75"></span>
          <span className="relative inline-flex rounded-full h-2.5 w-2.5 bg-emerald-500"></span>
        </span>
        llama.cpp server
      </div>
      <div className="flex gap-3 text-xs font-mono bg-slate-50 px-2.5 py-1 rounded border border-slate-100">
        {size && <span className="text-slate-600">SIZE:{size}</span>}
        <span className="text-slate-600">LOAD:{loadS}s</span>
      </div>
      <span className="text-xs text-slate-400 ml-auto uppercase tracking-wider font-semibold">Ready</span>
    </div>
  );
}
