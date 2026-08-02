import { LeakBanner } from "./LeakBanner";
import { LlamaServerReadout } from "./server/LlamaServerReadout";
import { EvalRunPanel } from "./evalrun/EvalRunPanel";

/// Inspector ("Latency") view — the TEST-RUN cost page: per-task latency/cache/memory of
/// the current Test-page batch, keyed by the (collection, task, model) triple on the
/// streamed events, live while a run streams. Workspace-prompt per-token timing moved
/// wholly under the Analysis tab (it renders the identical `LatencyTimelines` below each
/// answer) — one surface per question, no duplicate.
export function InspectorPage() {
  return (
    <div className="flex flex-col h-full bg-white text-slate-900" data-testid="inspector">
      {/* Header section for the dashboard */}
      <div className="px-6 py-5 border-b border-slate-200 bg-slate-50 flex-none">
        <h1 className="text-xl font-semibold tracking-tight text-slate-900">Infrastructure \u0026 Latency</h1>
        <p className="text-sm text-slate-500 mt-1">Live hardware telemetry and performance metrics for your local LLM \u0026 GPU cluster.</p>
      </div>
      
      {/* Scrollable content area */}
      <div className="flex-1 overflow-y-auto px-6 py-6 space-y-8">
        <LlamaServerReadout />
        <LeakBanner />
        <EvalRunPanel />
      </div>
    </div>
  );
}
