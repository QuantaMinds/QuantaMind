import type { CompareRow, RowStatus } from "../state/compareStore";
import { useModelLabel } from "../../models/hooks/useModelLabel";
import { cacheReuse } from "../../../shared/format/cache";

type Props = { row: CompareRow };

const STATUS_LABEL: Record<RowStatus, string> = {
  pending: "Waiting",
  loading: "Loading model…",
  running: "Running",
  done: "Done",
  cancelled: "Cancelled",
  error: "Error",
};

const STATUS_CLASS: Record<RowStatus, string> = {
  pending: "text-gray-500",
  loading: "text-blue-600 animate-pulse",
  running: "text-blue-600 animate-pulse",
  done: "text-green-600 font-semibold",
  cancelled: "text-amber-600",
  error: "text-red-600 font-semibold",
};

const formatMetrics = (m: NonNullable<CompareRow["metrics"]>): string => {
  const ttft = m.ttft_ms != null ? `TTFT ${m.ttft_ms}ms` : null;
  const tps = m.tokens_per_sec != null ? `${m.tokens_per_sec.toFixed(1)} tok/s` : null;
  const tokens = `${m.token_count} tokens`;
  // Purely additive: the prefix-cache segment appends ONLY for a llama.cpp run that
  // reported cache reuse (`available`). For a remote backend it's absent, so the string is
  // byte-identical to before — never a false "0 reused" on a backend without the feature.
  const cr = cacheReuse(m.stats?.cache_n, m.stats?.prompt_eval_count);
  const cache = cr.available ? `cache ${cr.cached}/${cr.cached + cr.recomputed} reused` : null;
  return [ttft, tps, tokens, cache].filter(Boolean).join(" · ");
};

export function CompareColumn({ row }: Props) {
  const label = useModelLabel();
  return (
    <div
      data-testid={`compare-column-${row.model}`}
      className="border border-gray-100 rounded-lg p-3 flex flex-col gap-2 min-w-[380px] max-w-[820px] flex-1 bg-gray-50 font-mono"
    >
      <div className="flex items-center justify-between gap-4 border-b border-gray-100 pb-1.5">
        <span className="text-xs font-bold text-gray-700 break-all">{label(row.model)}</span>
        <span
          data-testid={`compare-status-${row.model}`}
          className={`text-xs ${STATUS_CLASS[row.status]}`}
        >
          [{STATUS_LABEL[row.status]}]
        </span>
      </div>
      {row.status === "loading" && !row.output ? (
        <div
          data-testid={`compare-loading-${row.model}`}
          className="flex items-center gap-2 text-xs text-gray-500 bg-gray-100 border border-gray-100 rounded p-2.5 min-h-[280px] select-none"
        >
          <span aria-hidden className="inline-block w-3 h-3 border-2 border-gray-400 border-t-transparent rounded-full animate-spin" />
          <span>Loading model… (up to 30s)</span>
        </div>
      ) : (
        <pre
          data-testid={`compare-output-${row.model}`}
          className="text-xs whitespace-pre-wrap break-words font-mono bg-gray-100 border border-gray-100 rounded p-2.5 min-h-[280px] max-h-[60vh] overflow-auto text-gray-900"
        >
          {row.output || (row.status === "pending" ? "" : " ")}
        </pre>
      )}
      {row.metrics && row.status === "done" && (
        <div className="text-xs text-gray-500 pt-1 select-none font-semibold" data-testid={`compare-metrics-${row.model}`}>
          {formatMetrics(row.metrics)}
        </div>
      )}
      {row.error && (
        <div role="alert" data-testid={`compare-error-${row.model}`} className="text-xs text-red-600 font-semibold pt-1">
          {row.error.kind}: {row.error.message}
        </div>
      )}
    </div>
  );
}
