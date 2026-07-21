import { LeakBanner } from "./LeakBanner";
import { LlamaServerReadout } from "./server/LlamaServerReadout";
import { EvalRunPanel } from "./evalrun/EvalRunPanel";
import { SttInspectorSection } from "../../sttInspector/components/SttInspectorSection";
import { useParentWidth } from "../hooks/useParentWidth";

/// Inspector ("Latency") view — the TEST-RUN cost page: per-task latency/cache/memory of
/// the current Test-page batch, keyed by the (collection, task, model) triple on the
/// streamed events, live while a run streams. Workspace-prompt per-token timing moved
/// wholly under the Analysis tab (it renders the identical `LatencyTimelines` below each
/// answer) — one surface per question, no duplicate.
export function InspectorPage() {
  const [ref, width] = useParentWidth<HTMLDivElement>();
  return (
    <div className="space-y-4" data-testid="inspector" ref={ref}>
      <LlamaServerReadout />
      <LeakBanner />
      <EvalRunPanel />
      <SttInspectorSection width={width} />
    </div>
  );
}
