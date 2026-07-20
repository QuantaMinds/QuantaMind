import { useEffect, useRef, useState } from "react";
import { useCompareStore } from "../../compare/state/compareStore";
import { useNavStore } from "../../../shared/state/navStore";
import { LeakBanner } from "./LeakBanner";
import { LlamaServerReadout } from "./server/LlamaServerReadout";
import { LatencyTimelines } from "./timeline/LatencyTimelines";
import { EvalRunPanel } from "./evalrun/EvalRunPanel";
import { SttInspectorSection } from "../../sttInspector/components/SttInspectorSection";
import { useSttResultStore } from "../../sttInspector/state/sttResultStore";
import { useBatchStore } from "../../eval/state/batchStore";
import { useParentWidth } from "../hooks/useParentWidth";

type Source = "workspace" | "test";

const srcBtn = (active: boolean) =>
  active
    ? "bg-white text-slate-900 shadow-sm border border-slate-200/60 px-3 py-1 text-xs font-semibold rounded-md"
    : "px-3 py-1 text-xs font-medium text-slate-600 hover:text-slate-900 rounded-md";

/// Inspector ("Latency") view, two sources behind one toggle:
/// - Workspace prompt: per-token timing of the last Workspace/Analysis run (LatencyTimelines,
///   shared with the Analysis tab).
/// - Test run: per-task latency/cache/memory of the current Test-page batch, keyed by the
///   (collection, task, model) triple on the streamed events — live while a run streams.
export function InspectorPage() {
  const rows = useCompareStore((s) => s.rows);
  const topView = useNavStore((s) => s.topView);
  const batchRunning = useBatchStore((s) => s.running);
  const [ref, width] = useParentWidth<HTMLDivElement>();
  const [source, setSource] = useState<Source>("workspace");
  // Auto-bind to the live batch ONCE per run start (the not-running → running transition),
  // so a user who switches back to Workspace mid-run isn't clobbered (the effect is keyed
  // on the transition, not the value — see the pre-fill rule in docs/process.md).
  const wasRunning = useRef(false);
  useEffect(() => {
    if (batchRunning && !wasRunning.current) setSource("test");
    wasRunning.current = batchRunning;
  }, [batchRunning]);

  const charted = rows.filter((r) => (r.metrics?.timeline?.length ?? 0) > 0);
  const hasStt = useSttResultStore((s) => s.result != null);

  const toggle = (
    <div className="flex items-center gap-1 bg-slate-100/70 p-1 rounded-lg w-fit" role="tablist" data-testid="latency-source-toggle">
      <button type="button" role="tab" aria-selected={source === "workspace"} className={srcBtn(source === "workspace")}
        onClick={() => setSource("workspace")} data-testid="latency-source-workspace">
        Workspace prompt
      </button>
      <button type="button" role="tab" aria-selected={source === "test"} className={srcBtn(source === "test")}
        onClick={() => setSource("test")} data-testid="latency-source-test">
        Test run
      </button>
    </div>
  );

  if (source === "test") {
    return (
      <div className="space-y-4" data-testid="inspector" ref={ref}>
        <LlamaServerReadout />
        {toggle}
        <EvalRunPanel />
      </div>
    );
  }

  if (charted.length === 0 && !hasStt) {
    return (
      <div className="space-y-3" data-testid="inspector-empty" ref={ref}>
        <LlamaServerReadout />
        {toggle}
        <div className="text-sm text-gray-500 border rounded p-6 text-center">
          Run a prompt — or transcribe audio — in the Workspace to inspect timing.
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-4" data-testid="inspector" ref={ref}>
      <LlamaServerReadout />
      <LeakBanner />
      {toggle}
      {/* LatencyTimelines self-gates: renders nothing until a run carries a timeline. */}
      <LatencyTimelines active={topView === "inspector"} showExport />
      <SttInspectorSection width={width} />
    </div>
  );
}
