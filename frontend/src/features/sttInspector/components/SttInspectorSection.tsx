import { useSttResultStore } from "../state/sttResultStore";
import { buildConfidenceTimeline } from "../format/confidenceTimeline";
import { buildConfidenceHistogram } from "../format/confidenceHistogram";
import { ConfidenceTimeline, SEG_COLOR } from "./ConfidenceTimeline";
import { ConfidenceHistogram } from "./ConfidenceHistogram";
import { SttPhaseBar } from "./SttPhaseBar";
import { SttMetricCards } from "./SttMetricCards";
import { PipelineSummary } from "./PipelineSummary";

const LEGEND = [
  { kind: "ok" as const, label: "Confident" },
  { kind: "low" as const, label: "Low confidence" },
  { kind: "silenceOut" as const, label: "Speech over silence" },
];

/// STT Inspector section: the measured profile of the last transcription, rendered
/// with the same density as the LLM Inspector — wall-time phase bar, per-segment
/// confidence timeline, confidence distribution, and the metric-card grid. Renders
/// nothing until a transcription completes.
export function SttInspectorSection({ width }: { width: number }) {
  const t = useSttResultStore((s) => s.result);
  if (!t) return null;

  const chart = buildConfidenceTimeline(t.segments, t.audio.duration_secs);
  const buckets = buildConfidenceHistogram(chart.bars);
  const chartWidth = Math.max(320, width);

  return (
    <section className="bg-white space-y-5 border border-slate-200 rounded-xl p-5 shadow-sm" data-testid="stt-inspector-section">
      <div className="flex items-center justify-between border-b border-slate-100 pb-3">
        <div className="flex items-center gap-2">
          <svg className="w-5 h-5 text-slate-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 11a7 7 0 01-7 7m0 0a7 7 0 01-7-7m7 7v4m0 0H8m4 0h4m-4-8a3 3 0 01-3-3V5a3 3 0 116 0v6a3 3 0 01-3 3z" />
          </svg>
          <div className="text-base font-semibold text-slate-900 tracking-tight">
            Whisper.cpp STT Pipeline <span className="font-mono text-slate-400 font-normal ml-2 text-sm">— {t.model}</span>
          </div>
        </div>
        <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs font-medium text-slate-600 bg-slate-50 px-3 py-1.5 rounded-md border border-slate-100 shadow-sm">
          <span className="text-slate-400 uppercase tracking-wider text-[10px] font-bold self-center">Segments</span>
          {LEGEND.map((l) => (
            <span key={l.kind} className="flex items-center gap-1.5">
              <span className="inline-block h-2 w-2 rounded-full shadow-sm" style={{ background: SEG_COLOR[l.kind] }} />
              {l.label}
            </span>
          ))}
        </div>
      </div>

      <SttPhaseBar firstSegmentMs={t.stt_profile?.perf?.first_segment_ms ?? null} wallMs={t.stats.transcribe_wall_ms} width={chartWidth} />

      <ConfidenceTimeline chart={chart} width={chartWidth} height={150} />

      {buckets.length > 0 && (
        <div>
          <div className="text-[11px] font-bold text-gray-400 uppercase tracking-wider mb-1">Confidence distribution</div>
          <ConfidenceHistogram buckets={buckets} width={chartWidth} height={110} />
        </div>
      )}

      <SttMetricCards transcript={t} />

      <PipelineSummary />
    </section>
  );
}
