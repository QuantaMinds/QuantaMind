import { describe, expect, it } from "vitest";
import { InstalledModelInfoSchema } from "../storage";

const base = {
  name: "llama3.2:1b",
  size_bytes: 1000,
  modified_at: "2025-01-01",
  family: "llama",
  parameter_size: "1B",
  quantization: "Q4_K_M",
};

describe("InstalledModelInfoSchema backend field", () => {
  it("accepts an ollama-backed model", () => {
    const parsed = InstalledModelInfoSchema.parse({ ...base, backend: "ollama" });
    expect(parsed.backend).toBe("ollama");
  });

  it("accepts a llama_cpp-backed model", () => {
    const parsed = InstalledModelInfoSchema.parse({ ...base, backend: "llama_cpp" });
    expect(parsed.backend).toBe("llama_cpp");
  });

  it("accepts the remote vllm and sglang backends", () => {
    expect(InstalledModelInfoSchema.parse({ ...base, backend: "vllm" }).backend).toBe("vllm");
    expect(InstalledModelInfoSchema.parse({ ...base, backend: "sglang" }).backend).toBe("sglang");
  });

  it("rejects an unknown backend", () => {
    expect(() => InstalledModelInfoSchema.parse({ ...base, backend: "openai" })).toThrow();
  });

  it("rejects a missing backend", () => {
    expect(() => InstalledModelInfoSchema.parse(base)).toThrow();
  });
});
