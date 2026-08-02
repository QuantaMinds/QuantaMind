import { useLeakStore } from "../state/leakStore";
import { detectLeak } from "../format/leak";
import { formatBytes } from "../../../shared/format/bytes";

/// Global heuristic banner: watches the server-RSS series sampled per run and
/// warns if it climbs monotonically across the last 5 runs. Hidden until there
/// are enough samples to say anything.
export function LeakBanner() {
  const series = useLeakStore((s) => s.series);
  const v = detectLeak(series);
  if (v.samples < 5) return null;
  const latest = series[series.length - 1].rssBytes;
  return (
    <div
      data-testid="leak-banner"
      className={`text-xs rounded px-3 py-1.5 ${v.suspected ? "bg-amber-100 text-amber-800" : "bg-gray-100 text-gray-500"}`}
    >
      {v.suspected
        ? `⚠ Possible memory leak — Server RSS rose ${formatBytes(v.growthBytes)} across the last 5 same-model runs (now ${formatBytes(latest)}).`
        : `Server memory looks stable across recent runs (${formatBytes(latest)}).`}
    </div>
  );
}
