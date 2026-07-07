import type { Readiness } from "../../../shared/ipc/eval/readiness";

export function StatusBadge({ status }: { status: Readiness }) {
  if (status === "ready") {
    return (
      <span
        data-testid="readiness-badge-ready"
        className="inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-[11px] font-bold tracking-wide bg-emerald-50 text-emerald-700 border border-emerald-200 select-none shadow-sm transition-all duration-200"
      >
        <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 animate-pulse shrink-0" />
        READY
      </span>
    );
  }
  if (status === "not_ready") {
    return (
      <span
        data-testid="readiness-badge-not_ready"
        className="inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-[11px] font-bold tracking-wide bg-rose-50 text-rose-700 border border-rose-200 select-none shadow-sm transition-all duration-200"
      >
        <span className="w-1.5 h-1.5 rounded-full bg-rose-500 shrink-0" />
        NOT READY
      </span>
    );
  }
  // status === "conditional"
  return (
    <span
      data-testid="readiness-badge-conditional"
      className="inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-[11px] font-bold tracking-wide bg-amber-50 text-amber-700 border border-amber-200 select-none shadow-sm transition-all duration-200"
    >
      <span className="w-1.5 h-1.5 rounded-full bg-amber-500 shrink-0" />
      CONDITIONAL
    </span>
  );
}
