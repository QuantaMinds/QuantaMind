import type { HardwareSnapshot } from "../../../shared/ipc/compare/hardware";
import type { CompareRow, CompareModel } from "../state/compareStore";
import type { InstalledModelInfo } from "../../../shared/ipc/models/storage";
import type { AnalysisDocument, DocModel, DocRun } from "./schema";
import { ulid } from "./ulid";

const slug = (s: string) => s.replace(/[^a-zA-Z0-9]+/g, "_").replace(/^_|_$/g, "").toLowerCase();
const modelId = (name: string) => `model.${slug(name)}`;

export interface BuildReportInput {
  prompt: string;
  systemPrompt?: string;
  hardwareSnapshot: HardwareSnapshot | null;
  selectedModels: CompareModel[];
  rows: CompareRow[];
  installed?: InstalledModelInfo[];
  now?: () => Date;
  uid?: () => string;
}

/// Build the export document from whatever data we currently have. Fields we
/// can't populate are simply omitted (the schema keeps them optional).
export function buildReport(input: BuildReportInput): AnalysisDocument {
  const now = input.now ?? (() => new Date());
  const uid = input.uid ?? (() => ulid());
  const installed = input.installed ?? [];
  const info = (name: string) => installed.find((m) => m.name === name);
  const sizeOf = (name: string) => input.selectedModels.find((m) => m.name === name)?.size_bytes;

  const names = input.rows.length
    ? Array.from(new Set(input.rows.map((r) => r.model)))
    : input.selectedModels.map((m) => m.name);
  const models: DocModel[] = names.map((name) => {
    const d = info(name);
    const sz = sizeOf(name);
    return {
      id: modelId(name), name, display_name: d?.display_name ?? name,
      ...(d?.family ? { family: d.family } : {}),
      ...(d?.quantization ? { quantization: d.quantization } : {}),
      ...(sz != null ? { size_bytes: sz } : {}),
      ...(d?.backend ? { backend: d.backend } : {}),
    };
  });

  const prompts = [{
    id: "prompt.main",
    ...(input.systemPrompt && input.systemPrompt.trim() ? { system_prompt: input.systemPrompt } : {}),
    user_prompt: input.prompt,
  }];

  const runs: DocRun[] = input.rows.map((r, i) => ({
    id: `run.${i}`,
    prompt_id: "prompt.main",
    model_id: modelId(r.model),
    started_at: r.startedAt,
    completed_at: r.endedAt,
    status: r.status === "done" ? "completed" : r.status,
    metrics: r.metrics
      ? { ttft_ms: r.metrics.ttft_ms, tokens_per_second: r.metrics.tokens_per_sec, total_tokens_generated: r.metrics.token_count }
      : null,
    output: { text: r.output, truncated: false },
    warnings: [],
    errors: r.error ? [{ kind: r.error.kind, message: r.error.message }] : [],
  }));

  const hw = input.hardwareSnapshot;
  const environment = {
    ...(hw ? { memory: { total_bytes: hw.total_memory_bytes, available_bytes_at_start: hw.available_memory_bytes }, gpu: { unified_memory: hw.is_apple_silicon } } : {}),
    runtimes: [{ name: "llama_cpp" }],
  };

  const title = names.length <= 1 ? `Run: ${names[0] ?? "(no model)"}` : `Compare: ${names.join(" · ")}`;

  return {
    schema_version: "1.0.0",
    document_id: uid(),
    document_type: "bench-report",
    title,
    created_at: now().toISOString(),
    environment,
    models,
    prompts,
    runs,
    findings: [],
    verdicts: [],
    reproducibility: { deterministic: false, seed_strategy: "default-random", notes: "Seeds are not pinned; re-runs will vary." },
  };
}
