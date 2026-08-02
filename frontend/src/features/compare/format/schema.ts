
// The exported document shape — a populated subset of docs/reference.md#analysis-schema.
// Everything beyond the required spine is optional; we only emit what we have.

export interface DocModel {
  id: string;
  name: string;
  display_name?: string;
  family?: string;
  quantization?: string;
  size_bytes?: number;
  backend?: string;
}

export interface DocPrompt {
  id: string;
  system_prompt?: string;
  user_prompt: string;
}

export interface DocRunMetrics {
  ttft_ms: number | null;
  tokens_per_second: number | null;
  total_tokens_generated: number;
}

export interface DocRun {
  id: string;
  prompt_id: string;
  model_id: string;
  started_at: string | null;
  completed_at: string | null;
  status: string;
  metrics: DocRunMetrics | null;
  output: { text: string; truncated: boolean };
  warnings: string[];
  errors: { kind: string; message: string }[];
}

export interface DocEnvironment {
  memory?: { total_bytes: number; available_bytes_at_start: number };
  gpu?: { unified_memory: boolean };
  runtimes?: { name: string }[];
}

export interface AnalysisDocument {
  schema_version: string;
  document_id: string;
  document_type: "bench-report" | "analysis";
  title: string;
  created_at: string;
  environment?: DocEnvironment;
  models: DocModel[];
  prompts: DocPrompt[];
  runs: DocRun[];
  findings: unknown[];
  verdicts: unknown[];
  reproducibility: { deterministic: boolean; seed_strategy: string; notes: string };
}
