import { useState } from "react";
import type { RunSummary } from "../../../../shared/ipc/eval/matrix";

const W = 620;
const H = 200;
const ML = 48; // left margin (y label + % ticks)
const MR = 16;
const MT = 16;
const MB = 46; // bottom margin (x label)
const PW = W - ML - MR;
const PH = H - MT - MB;
const COLORS = ["#60a5fa", "#f472b6", "#4ade80", "#fbbf24", "#a78bfa", "#22d3ee"];

type MetricKind = "composite" | "Pass^k";

interface Point {
  model: string;
  i: number;
  v: number;
  ts: string;
  kind: MetricKind;
  x: number;
  y: number;
  color: string;
}

/// Group history into one score-over-runs series per model, in record order, plus a
/// count of runs dropped for having neither a composite nor a Pass^k (so the chart can
/// say so instead of a silently shorter line).
function seriesByModel(history: RunSummary[]): {
  series: Array<{ model: string; points: Array<{ v: number; ts: string; kind: MetricKind }> }>;
  dropped: number;
} {
  const order: string[] = [];
  const byModel = new Map<string, Array<{ v: number; ts: string; kind: MetricKind }>>();
  let dropped = 0;
  for (const h of history) {
    // Single-turn runs plot their composite; agentic-only runs (no composite)
    // plot their Pass^k rate so batch runs still show on the timeline.
    const kind: MetricKind = h.composite != null ? "composite" : "Pass^k";
    const v = h.composite ?? h.pass_k ?? null;
    if (v == null) {
      dropped += 1;
      continue;
    }
    if (!byModel.has(h.model)) {
      byModel.set(h.model, []);
      order.push(h.model);
    }
    byModel.get(h.model)!.push({ v, ts: h.ts, kind });
  }
  return { series: order.map((model) => ({ model, points: byModel.get(model)! })), dropped };
}

/// Interactive pure-SVG regression chart: composite score (y) over consecutive
/// runs (x), one line per model. Hover a point for its exact value; the axes are
/// labelled so the meaning is explicit.
export function HistoryTimeline({ history }: { history: RunSummary[] }) {
  const [hover, setHover] = useState<Point | null>(null);
  const { series, dropped } = seriesByModel(history);

  if (series.length === 0) {
    return (
      <div style={{ padding: 30, textAlign: "center", color: "#475569", fontSize: 13, fontFamily: "Inter,sans-serif" }} data-testid="eval-history-empty">
        No run history yet — run the collection to start tracking regressions.
      </div>
    );
  }

  // Each model spans the plot on its OWN run ordinal (oldest→newest) — NOT a shared
  // global length, which left-packed models with fewer runs against the longest model's
  // axis. A single-run series sits at the left edge (its run #1), never dead-center.
  const xFor = (i: number, len: number) => (len <= 1 ? ML : ML + (i * PW) / (len - 1));
  // Clamp to [0,1] so a corrupt out-of-range score pins to an axis edge, not off-canvas.
  const yFor = (v: number) => MT + (1 - Math.min(1, Math.max(0, v))) * PH;

  const points: Point[][] = series.map((s, si) =>
    s.points.map((p, i) => ({ model: s.model, i, v: p.v, ts: p.ts, kind: p.kind, x: xFor(i, s.points.length), y: yFor(p.v), color: COLORS[si % COLORS.length] })),
  );

  // Clamp the tooltip box inside the plot.
  const TT_W = 150;
  const TT_H = 38;
  const ttX = hover ? Math.min(Math.max(hover.x + 10, ML), W - MR - TT_W) : 0;
  const ttY = hover ? Math.min(Math.max(hover.y - TT_H - 6, MT), H - MB - TT_H) : 0;

  return (
    <div style={{ padding: "12px 20px" }} data-testid="eval-history-timeline">
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" height={H} role="img" aria-label="Score (composite or Pass^k) over runs">
        {/* gridlines + y ticks */}
        {[0, 0.5, 1].map((g) => (
          <g key={g}>
            <line x1={ML} y1={yFor(g)} x2={W - MR} y2={yFor(g)} stroke="rgba(0,0,0,0.06)" />
            <text x={ML - 6} y={yFor(g) + 3} textAnchor="end" fill="#64748b" fontSize={9} fontFamily="Inter,sans-serif">{Math.round(g * 100)}%</text>
          </g>
        ))}

        {/* axis labels (what the axes mean) */}
        <text transform={`translate(12 ${MT + PH / 2}) rotate(-90)`} textAnchor="middle" fill="#64748b" fontSize={10} fontFamily="Inter,sans-serif">
          Score (%) — composite or Pass^k
        </text>
        <text x={ML + PW / 2} y={H - 8} textAnchor="middle" fill="#64748b" fontSize={10} fontFamily="Inter,sans-serif">
          Run # — oldest → newest
        </text>

        {/* series */}
        {points.map((pts) => (
          <g key={pts[0]?.model ?? Math.random()}>
            <polyline fill="none" stroke={pts[0]?.color} strokeWidth={2} points={pts.map((p) => `${p.x},${p.y}`).join(" ")} />
            {pts.map((p) => (
              <circle key={p.i} cx={p.x} cy={p.y} r={hover && hover.model === p.model && hover.i === p.i ? 4.5 : 2.5} fill={p.color} />
            ))}
            {/* larger transparent hit targets for easy hovering */}
            {pts.map((p) => (
              <circle
                key={`hit-${p.i}`}
                cx={p.x}
                cy={p.y}
                r={9}
                fill="transparent"
                style={{ cursor: "pointer" }}
                onMouseEnter={() => setHover(p)}
                onMouseLeave={() => setHover((h) => (h?.model === p.model && h?.i === p.i ? null : h))}
              />
            ))}
          </g>
        ))}

        {/* hover crosshair + tooltip */}
        {hover && (
          <g pointerEvents="none">
            <line x1={hover.x} y1={MT} x2={hover.x} y2={MT + PH} stroke="rgba(0,0,0,0.12)" strokeDasharray="3 3" />
            <rect x={ttX} y={ttY} width={TT_W} height={TT_H} rx={6} fill="#1e293b" stroke="rgba(0,0,0,0.1)" />
            <text x={ttX + 9} y={ttY + 15} fill="#f8fafc" fontSize={10} fontWeight={600} fontFamily="Inter,sans-serif">{hover.model}</text>
            <text x={ttX + 9} y={ttY + 29} fill="#93c5fd" fontSize={10} fontFamily="Inter,sans-serif">
              Run {hover.i + 1}: {Math.round(hover.v * 100)}% {hover.kind}
            </text>
          </g>
        )}
      </svg>

      {/* Legend */}
      <div style={{ display: "flex", flexWrap: "wrap", gap: 12, marginTop: 8 }}>
        {series.map((s, si) => (
          <div key={s.model} style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <span style={{ width: 10, height: 10, borderRadius: 2, background: COLORS[si % COLORS.length], display: "inline-block" }} />
            <span style={{ fontSize: 11, color: "#475569", fontFamily: "Inter,sans-serif" }}>{s.model}</span>
          </div>
        ))}
      </div>

      {dropped > 0 && (
        <div data-testid="eval-history-dropped" style={{ marginTop: 6, fontSize: 11, color: "#94a3b8", fontFamily: "Inter,sans-serif" }}>
          {dropped} run{dropped === 1 ? "" : "s"} had no composite or Pass^k score — not plotted.
        </div>
      )}
    </div>
  );
}

