import { describe, it, expect } from "vitest";
import { buildReport } from "../format/buildReport";

const FIXED = () => new Date("2026-05-23T14:01:22.000Z");
const UID = () => "01TESTULIDFIXED0000000000A";

describe("buildReport (analysis document v1)", () => {
  it("emits the document spine: version, id, type, created_at, strategy", () => {
    const r = buildReport({
      prompt: "hi", hardwareSnapshot: null,
      selectedModels: [], rows: [], now: FIXED, uid: UID,
    });
    expect(r.schema_version).toBe("1.0.0");
    expect(r.document_id).toBe("01TESTULIDFIXED0000000000A");
    expect(r.document_type).toBe("bench-report");
    expect(r.created_at).toBe("2026-05-23T14:01:22.000Z");
    expect(r.findings).toEqual([]);
    expect(r.verdicts).toEqual([]);
    expect(r.reproducibility.deterministic).toBe(false);
  });

  it("builds a model + run from a finished row, enriched from installed metadata", () => {
    const r = buildReport({
      prompt: "p", systemPrompt: "sys", hardwareSnapshot: null,
      selectedModels: [{ name: "a", size_bytes: 2_000_000_000 }],
      installed: [{ name: "a", size_bytes: 2_000_000_000, modified_at: "", family: "llama",
        parameter_size: "1B", quantization: "Q4_K_M", backend: "llama_cpp" }],
      rows: [{ model: "a", modelId: "u", status: "done", output: "ok",
        metrics: { ttft_ms: 10, tokens_per_sec: 30, token_count: 5 },
        error: null, startedAt: "s", endedAt: "e" }],
      now: FIXED, uid: UID,
    });
    expect(r.models[0]).toMatchObject({
      id: "model.a", name: "a", family: "llama", quantization: "Q4_K_M",
      size_bytes: 2_000_000_000, backend: "llama_cpp",
    });
    expect(r.prompts[0]).toMatchObject({ id: "prompt.main", system_prompt: "sys", user_prompt: "p" });
    expect(r.runs[0]).toMatchObject({
      id: "run.0", prompt_id: "prompt.main", model_id: "model.a", status: "completed",
      metrics: { ttft_ms: 10, tokens_per_second: 30, total_tokens_generated: 5 },
      output: { text: "ok", truncated: false }, errors: [],
    });
  });

  it("maps an errored row into runs[].errors and leaves metrics null", () => {
    const r = buildReport({
      prompt: "p", hardwareSnapshot: null, selectedModels: [],
      rows: [{ model: "qwen", modelId: null, status: "error", output: "",
        metrics: null, error: { kind: "inference", message: "HTTP 404" }, startedAt: null, endedAt: null }],
      now: FIXED, uid: UID,
    });
    expect(r.runs[0].status).toBe("error");
    expect(r.runs[0].metrics).toBeNull();
    expect(r.runs[0].errors).toEqual([{ kind: "inference", message: "HTTP 404" }]);
  });

  it("captures hardware + unified-memory into the environment when available", () => {
    const r = buildReport({
      prompt: "p",
      hardwareSnapshot: { total_memory_bytes: 16, available_memory_bytes: 8, is_apple_silicon: true },
      selectedModels: [], rows: [], now: FIXED, uid: UID,
    });
    expect(r.environment).toMatchObject({
      memory: { total_bytes: 16, available_bytes_at_start: 8 },
      gpu: { unified_memory: true },
      runtimes: [{ name: "llama_cpp" }],
    });
  });

  it("preserves run order from the store", () => {
    const r = buildReport({
      prompt: "p", hardwareSnapshot: null, selectedModels: [],
      rows: ["c", "a", "b"].map((m) => ({
        model: m, modelId: null, status: "pending" as const, output: "",
        metrics: null, error: null, startedAt: null, endedAt: null,
      })),
      now: FIXED, uid: UID,
    });
    expect(r.runs.map((x) => x.model_id)).toEqual(["model.c", "model.a", "model.b"]);
  });
});
