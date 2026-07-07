import type { ModelVerdict } from "../../../shared/ipc/eval/readiness";

/// The agentic-aware recommendation (Phase 7.3). The backend returns verdicts
/// ranked best-first (Ready > Conditional > NotReady, ties by effort then steps),
/// so `verdicts[0]` IS the pick — this banner just frames it honestly: a clear
/// recommendation when something is Ready, a caveated "best available" when only
/// Conditional, and a "no model is ready — closest" when nothing qualifies (never a
/// fabricated Ready).
export function RecommendationBanner({ verdicts, profileName }: { verdicts: ModelVerdict[]; profileName: string }) {
  const pick = verdicts[0];
  if (!pick) return null;

  const status = pick.verdict.status;
  const reason = pick.verdict.blocking[0] ?? pick.verdict.conditions[0] ?? null;

  const config =
    status === "ready"
      ? {
          bg: "bg-emerald-50/50 border-emerald-200/80 text-emerald-900 border-l-emerald-500",
          statusCls: "bg-emerald-100/80 text-emerald-800 border-emerald-200/80",
          icon: (
            <svg className="w-5 h-5 text-emerald-600 shrink-0 mt-0.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
          ),
        }
      : status === "conditional"
        ? {
            bg: "bg-amber-50/50 border-amber-200/80 text-amber-900 border-l-amber-500",
            statusCls: "bg-amber-100/80 text-amber-800 border-amber-200/80",
            icon: (
              <svg className="w-5 h-5 text-amber-600 shrink-0 mt-0.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
              </svg>
            ),
          }
        : {
            bg: "bg-rose-50/50 border-rose-200/80 text-rose-900 border-l-rose-500",
            statusCls: "bg-rose-100/80 text-rose-800 border-rose-200/80",
            icon: (
              <svg className="w-5 h-5 text-rose-600 shrink-0 mt-0.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M10 14l2-2m0 0l2-2m-2 2l-2-2m2 2l2 2M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
              </svg>
            ),
          };

  const headline =
    status === "ready"
      ? `Recommended for ${profileName} on your hardware:`
      : status === "conditional"
        ? `Best available for ${profileName} (with caveats):`
        : `No model is ready for ${profileName} — closest:`;

  const tail =
    status === "ready"
      ? reason
        ? `Note: ${reason}`
        : "Meets every gate for this profile"
      : reason
        ? reason
        : "No blocking reason recorded";

  return (
    <div
      data-testid="recommendation-banner"
      data-status={status}
      className={`flex items-start gap-3.5 border border-l-4 rounded-xl p-3.5 text-sm shadow-sm transition-all duration-300 font-sans ${config.bg}`}
    >
      {config.icon}
      <div className="flex flex-col flex-1 gap-2.5">
        <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1 mt-0.5">
          <span className="font-medium text-slate-800">{headline}</span>
          <strong
            data-testid="recommendation-model"
            className="font-mono font-bold text-slate-900 border border-slate-200 bg-white/80 rounded px-2 py-0.5 text-[13px] shadow-3xs translate-y-[1px]"
          >
            {pick.model}
          </strong>
        </div>
        <div className="flex items-center gap-2">
          <span className={`inline-flex items-center px-2 py-0.5 rounded text-[10px] uppercase tracking-wider font-bold border ${config.statusCls}`}>
            {statusLabel(status)}
          </span>
          <span className="text-slate-600 text-[13px] font-medium bg-white/60 px-2 py-0.5 rounded border border-slate-200/50 shadow-3xs">
            {tail}
          </span>
        </div>
        {pick.memory?.estimated && (
          <div
            data-testid="recommendation-estimated"
            className="text-[11px] text-slate-500 font-medium flex items-center gap-1.5 mt-2 opacity-85"
          >
            <svg className="w-3.5 h-3.5 text-slate-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            VRAM usage is a conservative estimate due to incomplete model metadata.
          </div>
        )}
      </div>
    </div>
  );
}

function statusLabel(status: ModelVerdict["verdict"]["status"]): string {
  return status === "ready" ? "Ready" : status === "conditional" ? "Conditional" : "NotReady";
}

