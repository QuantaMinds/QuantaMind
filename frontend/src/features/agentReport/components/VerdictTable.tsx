import React from "react";
import type { AgentPath, MemoryProfile, ModelVerdict, ReadinessVerdict, Tier } from "../../../shared/ipc/eval/readiness";
import type { BackendKind } from "../../../shared/ipc/models/storage";
import { StatusBadge } from "./StatusBadge";
import { parseQuant } from "../../models/parse_quant";

export const PATH_LABEL: Record<AgentPath, string> = {
  prompt_based: "Prompt-Based",
  native_fc: "Native FC",
};

const TIER_RANK: Record<Tier, number> = { easy: 0, medium: 1, hard: 2, extreme: 3 };
const capitalize = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

/// Graduated readiness: "cleared X / requires Y". Hidden for an untiered profile
/// (required_tier absent or Easy) so single-tier collections stay uncluttered.
function TierLine({ verdict }: { verdict: ReadinessVerdict }) {
  const required = verdict.required_tier;
  if (!required || required === "easy") return null;
  const cleared = verdict.cleared_tier ?? null;
  const met = cleared != null && TIER_RANK[cleared] >= TIER_RANK[required];
  return (
    <div
      data-testid="tier-line"
      className={`text-[11px] font-semibold mt-1 ${met ? "text-emerald-600" : "text-amber-600"}`}
    >
      {met ? "✓" : "▸"} cleared {cleared ? capitalize(cleared) : "none"} / requires {capitalize(required)}
    </div>
  );
}

const gb = (bytes: number) => (bytes / 1024 ** 3).toFixed(1);

function modelQuant(m: ModelVerdict): string {
  if (m.quantization && m.quantization.trim()) return m.quantization.toUpperCase();
  const parsed = parseQuant(m.model);
  if (parsed) return parsed.toUpperCase();
  const match = m.model.match(/(q\d_[k01a-z_]+|\bbf16\b|\bf16\b)/i);
  if (match) return match[0].toUpperCase();
  return "—";
}

const pct = (x: number | null | undefined) => (x == null ? "N/A" : `${Math.round(x * 100)}%`);
const num1 = (x: number | null | undefined) => (x == null ? "N/A" : x.toFixed(1));
const tok = (x: number | null | undefined) => (x == null ? "N/A" : `${Math.round(x)} tok`);

function cliffLabel(c: ModelVerdict["cliff"]): string {
  if (!c || c.status === "NotProbed") return "N/A";
  if (c.status === "NoCliff") return `✓ No cliff (≥${c.tested.toLocaleString()} tok)`;
  if (c.status === "Broken") return "fails from start";
  return `Collapsed at ${c.depth.toLocaleString()} tok`;
}

function cliffColor(c: ModelVerdict["cliff"]): string {
  if (!c || c.status === "NotProbed") return "text-slate-800";
  return c.status === "NoCliff" ? "text-emerald-600" : "text-rose-600";
}

function getIndicatorLabel(reason: string): string {
  const lower = reason.toLowerCase();
  if (
    lower.includes("pass^k") ||
    lower.includes("pass") ||
    lower.includes("fail") ||
    lower.includes("false") ||
    lower.includes("'done'") ||
    lower.includes("hallucinat")
  )
    return "Reliability";
  if (lower.includes("loop") || lower.includes("infinite")) return "Loops";
  if (lower.includes("cliff") || lower.includes("context") || lower.includes("token")) return "Context";
  if (lower.includes("vram") || lower.includes("memory") || lower.includes("pressure") || lower.includes("fit") || lower.includes("offload")) return "Hardware";
  if (lower.includes("native") || lower.includes("tool-calling")) return "Native FC";
  if (lower.includes("error")) return "Run Error";
  if (lower.includes("slow") || lower.includes("latency") || lower.includes("ms") || lower.includes("speed")) return "Performance";
  if (lower.includes("step") || lower.includes("efficiency") || lower.includes("effort")) return "Efficiency";
  return "System";
}

function getDetailsLine(v: ModelVerdict, profileMinPassK?: number): string {
  const details: string[] = [];
  
  const passReason = v.verdict.blocking.find(b => b.toLowerCase().includes("pass"));
  if (passReason) {
    const match = passReason.match(/(\d+\.\d+)\s*<\s*(\d+\.\d+)/);
    if (match) {
      details.push(`Pass^k (${match[1]}) < ${match[2]}`);
    } else if (v.pass_k != null) {
      const target = profileMinPassK ?? 0.80;
      details.push(`Pass^k (${v.pass_k.toFixed(2)}) < ${target.toFixed(2)}`);
    }
  }
  
  if (v.cliff && v.cliff.status === "Collapsed") {
    details.push(`Reasoning Cliff (${v.cliff.depth})`);
  } else if (v.cliff && v.cliff.status === "Broken") {
    details.push(`Cliff fails from start`);
  }
  
  for (const b of v.verdict.blocking) {
    const lower = b.toLowerCase();
    if (!lower.includes("pass") && !lower.includes("cliff") && !lower.includes("context")) {
      const clean = b.charAt(0).toUpperCase() + b.slice(1);
      details.push(clean);
    }
  }
  
  return details.length > 0 ? `Details: ${details.join(" | ")}` : "";
}

function getConditionalBreakdown(v: ModelVerdict): string[] {
  const parts: string[] = [];
  
  if (v.memory?.pressure) {
    parts.push("! High Pressure");
  }
  
  for (const c of v.verdict.conditions) {
    const lower = c.toLowerCase();
    if (lower.includes("slow") || lower.includes("latency") || lower.includes("ms")) {
      const msMatch = c.match(/(\d+)\s*ms/);
      const targetMatch = c.match(/>\s*(\d+)\s*ms/);
      if (msMatch && targetMatch) {
        const ms = msMatch[1];
        const targetSec = Math.round(Number(targetMatch[1]) / 1000);
        parts.push(`! Latency (${ms}ms > ${targetSec}s)`);
      } else {
        parts.push(`! Latency (${c})`);
      }
    } else if (lower.includes("step") || lower.includes("efficiency") || lower.includes("limit")) {
      let clean = c;
      if (clean.includes(":")) {
        clean = clean.split(":")[1].trim();
      }
      parts.push(`! Efficiency (${clean})`);
    } else {
      parts.push(`! ${c}`);
    }
  }
  
  return parts;
}

const ctxLabel = (n: number) => (n >= 1024 ? `${Math.round(n / 1024)}k` : `${n}`);

function MemoryLine({ m, backend }: { m: MemoryProfile | null | undefined; backend: BackendKind }) {
  const getExpectedText = () => {
    if (!m) {
      if (backend !== "ollama") {
        return "VRAM fit: N/A (single-model backend)";
      }
      return "";
    }
    const note = !m.fits ? "won't fit" : m.pressure ? "high VRAM pressure" : "fits";
    const est = m.estimated ? " · est." : "";
    return `VRAM: ${gb(m.total_bytes)} GB (${gb(m.weights_bytes)} model + ${gb(m.kv_cache_bytes)} cache @ ${ctxLabel(m.context_length)} ctx) ${m.fits ? "<" : ">"} ${gb(m.cap_bytes)} GB cap · ${note}${est}`;
  };

  const expectedText = getExpectedText();

  if (!m) {
    return (
      <div className="text-slate-500 font-medium hidden">
        {expectedText}
      </div>
    );
  }

  return (
    <div className="hidden">
      <span>{expectedText}</span>
      {m.estimated && (
        <span data-testid="vram-estimated">
          conservative estimate
        </span>
      )}
    </div>
  );
}

// The measured numbers for a verdict — shown for EVERY status (a not_ready row
// must still display its pass^k/steps/effort, not just the red blocking markers).
function MetricsLine({ v }: { v: ModelVerdict }) {
  return (
    <div
      data-testid="readiness-metrics"
      className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-slate-500 font-mono mt-1.5"
    >
      <span data-testid="metric-passk" title="Strict Pass^k: fraction of tasks where EVERY one of the k runs passed (the reliability gate)">
        Pass^k {pct(v.pass_k)}
      </span>
      {v.total_runs != null && v.total_runs > 0 && (
        <>
          <span className="text-slate-300">·</span>
          <span
            data-testid="metric-runs"
            title="Run-level pass rate: individual runs that passed. A 14/16 shows here even when the strict all-k Pass^k gate reads 0%."
          >
            {v.passes ?? 0}/{v.total_runs} runs ({Math.round(((v.passes ?? 0) / v.total_runs) * 100)}%)
          </span>
        </>
      )}
      <span className="text-slate-300">·</span>
      <span data-testid="metric-steps">{num1(v.avg_steps)} steps</span>
      <span className="text-slate-300">·</span>
      <span data-testid="metric-effort">{tok(v.effort)} effort</span>
      <span className="text-slate-300">·</span>
      <span data-testid="metric-cliff" className={cliffColor(v.cliff)}>
        cliff {cliffLabel(v.cliff)}
      </span>
    </div>
  );
}

function Reasons({ v, profileName, vramFits }: { v: ModelVerdict["verdict"]; profileName: string; vramFits: boolean | null }) {
  return (
    <div className="hidden">
      {v.blocking.length === 0 && v.conditions.length === 0 ? (
        <>
          <span>Meets all criteria</span>
          {vramFits === true && <div>✓ Fits completely in VRAM</div>}
          <div>✓ Meets all performance criteria for '{profileName}'</div>
        </>
      ) : (
        <>
          {v.blocking.map((b, i) => (
            <span key={`b${i}`}>✗ {b}</span>
          ))}
          {v.conditions.map((c, i) => (
            <span key={`c${i}`}>! {c}</span>
          ))}
        </>
      )}
    </div>
  );
}

export function VerdictTable({
  verdicts,
  profileName = "Coding Agent",
  showNativeFc = true,
  unified = false,
}: {
  verdicts: ModelVerdict[];
  profileName?: string;
  showNativeFc?: boolean;
  /// Apple-Silicon / shared-memory machines have no discrete VRAM — label memory
  /// "Unified memory" there, "VRAM" on a discrete GPU. From the hardware snapshot.
  unified?: boolean;
}) {
  const filtered = verdicts.filter((m) => showNativeFc || m.verdict.path !== "native_fc");
  const memLabel = unified ? "Unified memory" : "VRAM";

  return (
    <div className="bg-white border border-slate-200 rounded-xl shadow-md overflow-hidden" data-testid="readiness-verdict-table">
      <table className="w-full border-collapse">
        <thead>
          <tr className="border-b border-slate-200 bg-slate-50/70 select-none">
            <th className="text-left text-xs font-bold text-slate-700 uppercase tracking-wider px-6 py-4 w-[22%]">
              [Model Info]
            </th>
            <th className="text-left text-xs font-bold text-slate-700 uppercase tracking-wider px-6 py-4 w-[13%]">
              [Quant]
            </th>
            <th className="text-left text-xs font-bold text-slate-700 uppercase tracking-wider px-6 py-4 w-[20%]">
              [Status]
            </th>
            <th className="text-left text-xs font-bold text-slate-700 uppercase tracking-wider px-6 py-4">
              [Memory &amp; Diagnostic Breakdown]
            </th>
          </tr>
        </thead>
        <tbody className="divide-y divide-slate-200">
          {filtered.map((m) => {
            const status = m.verdict.status;
            return (
              <tr
                key={`${m.model}-${m.backend}-${m.verdict.path}`}
                data-testid={`readiness-row-${m.model}`}
                className="hover:bg-slate-50/30 transition-colors duration-150"
              >
                {/* 1. Model Info Column */}
                <td className="px-6 py-4.5 align-top">
                  <div className="font-mono font-bold text-slate-900 text-sm">
                    [{m.model}]
                  </div>
                  <div className="text-[11px] font-semibold text-slate-500 mt-1">
                    ({PATH_LABEL[m.verdict.path]})
                  </div>
                  <TierLine verdict={m.verdict} />
                </td>

                {/* 2. Quant Column */}
                <td className="px-6 py-4.5 align-top">
                  <span className="font-mono text-slate-700 text-sm font-semibold lowercase">
                    {modelQuant(m)}
                  </span>
                </td>

                {/* 3. Status Badge Column */}
                <td className="px-6 py-4.5 align-top">
                  <StatusBadge status={status} />
                </td>

                {/* 4. Memory & Diagnostic Breakdown Column */}
                <td className="px-6 py-4.5 align-top">
                  {/* MemoryLine + Reasons remain hidden (their content is shown by the
                      status-styled breakdown below); MetricsLine is now rendered visibly. */}
                  <div className="hidden" aria-hidden="true">
                    <MemoryLine m={m.memory} backend={m.backend} />
                    <Reasons v={m.verdict} profileName={profileName} vramFits={m.memory ? m.memory.fits : null} />
                  </div>

                  {/* Visible Styled diagnostics breakdown based on status */}
                  <div className="font-sans text-xs">
                    {status === "ready" && (
                      <div className="flex flex-wrap items-center gap-2 text-emerald-700 font-bold">
                        <span>{memLabel}: {m.memory ? `${gb(m.memory.total_bytes)}GB` : "N/A"}</span>
                        {m.memory && (
                          <>
                            <span className="text-slate-300">|</span>
                            <span>✓ Fits in {memLabel}</span>
                          </>
                        )}
                        <span className="text-slate-300">|</span>
                        <span>✓ Meets Perf. Targets</span>
                      </div>
                    )}

                    {status === "not_ready" && (
                      <div className="flex flex-col gap-1.5 font-bold text-rose-700">
                        <div className="flex flex-wrap items-center gap-2">
                          <span>{memLabel}: {m.memory ? `${gb(m.memory.total_bytes)}GB` : "N/A"}</span>
                          <span className="text-slate-300">|</span>
                          <span>
                            BLOCKING: {m.verdict.blocking.map(b => `[✗ ${getIndicatorLabel(b)}]`).join(" ")}
                          </span>
                        </div>
                        {getDetailsLine(m) && (
                          <div className="text-[11px] text-slate-500 font-medium font-mono mt-0.5">
                            {getDetailsLine(m)}
                          </div>
                        )}
                      </div>
                    )}

                    {status === "conditional" && (
                      <div className="flex flex-wrap items-center gap-2 text-amber-700 font-bold">
                        <span>{memLabel}: {m.memory ? `${gb(m.memory.total_bytes)}GB` : "N/A"}</span>
                        {getConditionalBreakdown(m).map((item, idx) => (
                          <React.Fragment key={idx}>
                            <span className="text-slate-300">|</span>
                            <span>{item}</span>
                          </React.Fragment>
                        ))}
                      </div>
                    )}
                    {/* The measured numbers — shown for every status, so a not_ready row
                        still reveals its pass^k/steps/effort instead of just a red ✗. */}
                    <MetricsLine v={m} />
                  </div>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
