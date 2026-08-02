import { formatBytes } from "../../../shared/format/bytes";
import type { AnalysisDocument, DocRun, DocRunMetrics } from "./schema";
import { REPORT_FOOTER } from "./branding";

const metricsLine = (m: DocRunMetrics): string => {
  const parts: string[] = [];
  if (m.ttft_ms != null) parts.push(`TTFT ${m.ttft_ms}ms`);
  if (m.tokens_per_second != null) parts.push(`${m.tokens_per_second.toFixed(1)} tok/s`);
  parts.push(`${m.total_tokens_generated} tokens`);
  return parts.join(" · ");
};

const hardwareLine = (d: AnalysisDocument): string | null => {
  const mem = d.environment?.memory;
  if (!mem) return null;
  const label = d.environment?.gpu?.unified_memory ? "Apple Silicon, unified" : "RAM";
  return `- Hardware: ${formatBytes(mem.total_bytes)} total · ${formatBytes(mem.available_bytes_at_start)} available (${label})`;
};

const selectedLine = (d: AnalysisDocument): string => {
  const names = d.models.map((m) => m.size_bytes != null ? `${m.name} (${formatBytes(m.size_bytes)})` : m.name);
  return `- Models (${d.models.length}): ${names.join(", ")}`;
};

const modelName = (d: AnalysisDocument, id: string) => d.models.find((m) => m.id === id)?.name ?? id;

const runSection = (d: AnalysisDocument, r: DocRun): string[] => {
  const lines: string[] = ["", "---", "", `## ${modelName(d, r.model_id)}`];
  if (r.started_at) lines.push(`- Started: ${r.started_at}`);
  if (r.completed_at) lines.push(`- Ended: ${r.completed_at}`);
  if (r.errors.length > 0) {
    lines.push(`- Error: ${r.errors[0].kind}: ${r.errors[0].message}`);
    return lines;
  }
  if (r.metrics) lines.push(`- Metrics: ${metricsLine(r.metrics)}`);
  if (r.status === "cancelled") lines.push(`- Status: cancelled`);
  lines.push("", r.output.text);
  return lines;
};

export function toMarkdown(d: AnalysisDocument): string {
  const lines: string[] = ["# QuantaMind Compare Report"];
  lines.push(`- Run at: ${d.created_at}`);
  const hw = hardwareLine(d);
  if (hw) lines.push(hw);
  lines.push(selectedLine(d));
  const p = d.prompts[0];
  lines.push("", "## Prompt");
  for (const ln of (p?.user_prompt ?? "").split("\n")) lines.push(`> ${ln}`);
  for (const r of d.runs) lines.push(...runSection(d, r));
  lines.push("", "---", "", `_${REPORT_FOOTER}_`);
  return lines.join("\n");
}
