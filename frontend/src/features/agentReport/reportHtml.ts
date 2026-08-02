import { esc } from "../inspector/report/sections";
import type { ModelVerdict, Readiness, ReadinessProfile } from "../../shared/ipc/eval/readiness";

const STYLE = `body{font:13px -apple-system,system-ui,sans-serif;color:#0f172a;max-width:860px;margin:24px auto;padding:0 16px}
h1{font-size:18px}.muted{color:#64748b;font-size:12px}
table{width:100%;border-collapse:collapse;margin-top:12px}
th{text-align:left;font-size:11px;text-transform:uppercase;letter-spacing:.05em;color:#475569;padding:8px;border-bottom:1px solid #e2e8f0}
td{font-size:13px;padding:10px 8px;vertical-align:top;border-bottom:1px solid #f1f5f9}
.block{color:#dc2626}.cond{color:#b45309}.ok{color:#16a34a}.mem{color:#475569;margin-bottom:4px}`;

const STATUS_LABEL: Record<Readiness, string> = { ready: "READY", conditional: "CONDITIONAL", not_ready: "NOT READY" };
const STATUS_COLOR: Record<Readiness, string> = { ready: "#16a34a", conditional: "#b45309", not_ready: "#dc2626" };

const gb = (bytes: number) => (bytes / 1024 ** 3).toFixed(1);

/// The memory footprint line for the export — numbers + static text only, so it
/// needs no escaping; N/A for remote backends, silent when unmeasured.
function memoryHtml(m: ModelVerdict): string {
  const mem = m.memory;
  if (!mem) {
    return m.backend !== "llama_cpp" ? `<div class=mem>VRAM fit: N/A (remote backend)</div>` : "";
  }
  const note = !mem.fits ? "won't fit" : mem.pressure ? "high VRAM pressure" : "fits";
  const est = mem.estimated ? " (conservative estimate)" : "";
  const ctx = mem.context_length >= 1024 ? `${Math.round(mem.context_length / 1024)}k` : `${mem.context_length}`;
  return (
    `<div class=mem>VRAM: ${gb(mem.total_bytes)} GB (${gb(mem.weights_bytes)} model + ${gb(mem.kv_cache_bytes)} cache @ ${ctx} ctx) ` +
    `${mem.fits ? "&lt;" : "&gt;"} ${gb(mem.cap_bytes)} GB cap · ${note}${est}</div>`
  );
}

function rowHtml(m: ModelVerdict): string {
  const v = m.verdict;
  const reasons =
    v.blocking.length || v.conditions.length
      ? [
          ...v.blocking.map((b) => `<div class=block>✗ ${esc(b)}</div>`),
          ...v.conditions.map((c) => `<div class=cond>! ${esc(c)}</div>`),
        ].join("")
      : `<div class=ok>✓ Meets all criteria</div>`;
  const path = v.path === "native_fc" ? "Native FC" : "Prompt-Based";
  return (
    `<tr><td><b>${esc(m.model)}</b><div class=muted>(${esc(path)})</div></td>` +
    `<td>${esc(m.backend)}</td>` +
    `<td style="color:${STATUS_COLOR[v.status]};font-weight:700">${STATUS_LABEL[v.status]}</td>` +
    `<td>${memoryHtml(m)}${reasons}</td></tr>`
  );
}

/// Build a single self-contained, offline HTML readiness one-pager (inline CSS,
/// utf-8, no external assets). Every interpolated string is escaped. Pure builder.
export function buildReadinessHtml(
  verdicts: ModelVerdict[],
  profile: ReadinessProfile,
  collectionId: string,
  generatedAtIso: string,
): string {
  const rows = verdicts.map(rowHtml).join("") || `<tr><td colspan=4 class=muted>No models assessed.</td></tr>`;
  const yn = (b: boolean) => (b ? "yes" : "no");
  return (
    `<!doctype html><html><head><meta charset="utf-8">` +
    `<title>QuantaMind Agent Readiness</title><style>${STYLE}</style></head><body>` +
    `<h1>Local Agent Readiness</h1>` +
    `<p class=muted>Collection “${esc(collectionId)}” · profile “${esc(profile.name)}” · generated ${esc(generatedAtIso)}</p>` +
    `<p class=muted>Min Pass^k ${Math.round(profile.min_pass_k * 100)}% · forbid loops ${yn(profile.forbid_infinite_loop)} · ` +
    `forbid fake-done ${yn(profile.forbid_hallucinated_completion)} · require full VRAM ${yn(profile.require_full_vram)}. ` +
    `Verdicts are measured against this profile, not objective truth.</p>` +
    `<table><thead><tr><th>Model</th><th>Backend</th><th>Status</th><th>Diagnostic reasons</th></tr></thead>` +
    `<tbody>${rows}</tbody></table>` +
    `</body></html>`
  );
}
