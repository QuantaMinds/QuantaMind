import { InfoButton } from "../../../../shared/ui/InfoButton";
import { READINESS_HELP } from "../../readinessHelp";
import { formatBytes } from "../../../../shared/format/bytes";
import type { RightSizingGroup, RightSizingPick } from "../../../../shared/ipc/eval/readiness";

/// A weights bar scaled to the group's largest weights, so baseline vs pick are
/// visually comparable. The precision label rides on the bar so a Q8-graded fit
/// is never silently read as an f16 one.
function WeightsBar({ pick, scaleMax }: { pick: RightSizingPick; scaleMax: number }) {
  const pct = scaleMax > 0 ? Math.round((pick.weights_bytes / scaleMax) * 100) : 0;
  const cond = pick.status === "conditional";
  return (
    <div className="flex items-center gap-2 text-[11px] font-mono" data-testid={`rs-pick-${pick.model}`}>
      <span className="w-40 truncate text-gray-700" title={pick.model}>
        {pick.model}
      </span>
      {pick.quantization && (
        <span className="px-1 rounded bg-gray-100 text-gray-600 text-[10px]">{pick.quantization}</span>
      )}
      <span className="flex-1 h-3 bg-gray-100 rounded overflow-hidden">
        <span className={`block h-full ${cond ? "bg-amber-400" : "bg-blue-500"}`} style={{ width: `${pct}%` }} />
      </span>
      <span className="text-gray-500 w-16 text-right">{formatBytes(pick.weights_bytes)}</span>
    </div>
  );
}

function Chip({ children, tone }: { children: React.ReactNode; tone: "good" | "bad" | "neutral" }) {
  const cls =
    tone === "good" ? "bg-emerald-50 text-emerald-700" : tone === "bad" ? "bg-rose-50 text-rose-700" : "bg-gray-100 text-gray-600";
  return <span className={`px-1.5 py-0.5 rounded text-[10px] font-semibold ${cls}`}>{children}</span>;
}

function GroupCard({ g }: { g: RightSizingGroup }) {
  const scaleMax = Math.max(g.baseline.weights_bytes, g.pick.weights_bytes, 1);
  const samePick = g.pick.model === g.baseline.model;
  const qDelta = g.quality_delta_pp;
  return (
    <div className="border border-gray-200 rounded-lg p-3 space-y-2" data-testid={`rs-group-${g.group}`}>
      <div className="text-[11px] font-semibold text-gray-600 uppercase tracking-wide">{g.group}</div>
      <div className="space-y-1.5">
        {!samePick && (
          <div className="text-[10px] text-gray-400 uppercase">baseline (largest)</div>
        )}
        <WeightsBar pick={g.baseline} scaleMax={scaleMax} />
        {!samePick && (
          <>
            <div className="text-[10px] text-gray-400 uppercase">
              recommended pick {g.pick_is_conditional ? "(conditional)" : "(ready)"}
            </div>
            <WeightsBar pick={g.pick} scaleMax={scaleMax} />
          </>
        )}
      </div>
      <div className="flex flex-wrap items-center gap-1.5">
        {samePick ? (
          <Chip tone="neutral">already the smallest usable variant</Chip>
        ) : (
          <>
            <Chip tone="good">−{g.size_reduction_pct.toFixed(0)}% size</Chip>
            {g.memory_reduction_pct != null && <Chip tone="good">−{g.memory_reduction_pct.toFixed(0)}% memory</Chip>}
            {qDelta != null ? (
              <Chip tone={qDelta >= 0 ? "good" : qDelta >= -5 ? "neutral" : "bad"}>
                Pass^k {qDelta >= 0 ? "+" : ""}
                {qDelta.toFixed(1)} pp
              </Chip>
            ) : (
              <Chip tone="neutral">Pass^k not measured</Chip>
            )}
          </>
        )}
      </div>
      {g.pick_is_conditional && !samePick && (
        <div className="text-[10px] text-amber-700 bg-amber-50 rounded px-2 py-1" data-testid={`rs-advisory-${g.group}`}>
          ⚠ The smaller pick is Conditional — verify its caveats before switching.
        </div>
      )}
      <div className="text-[10px] text-gray-400">{g.rationale}</div>
    </div>
  );
}

/// "Right-Sizing" — for each family with ≥2 assessed quants, the smallest variant
/// still usable on this hardware vs the largest, with measured percent reductions
/// (no dollars). Renders the backend hint when nothing was comparable.
export function RightSizingSection({ groups, hint }: { groups: RightSizingGroup[]; hint: string | null }) {
  if (groups.length === 0 && !hint) return null;
  return (
    <div className="space-y-3" data-testid="right-sizing">
      <div className="flex items-center gap-1.5">
        <h3 className="text-sm font-semibold text-gray-800">Right-Sizing</h3>
        <InfoButton {...READINESS_HELP.rightSizing} align="left" testId="right-sizing" />
      </div>
      {groups.length === 0 ? (
        <div className="text-[11px] text-gray-500" data-testid="rs-hint">
          {hint}
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
          {groups.map((g) => (
            <GroupCard key={g.group} g={g} />
          ))}
        </div>
      )}
    </div>
  );
}
