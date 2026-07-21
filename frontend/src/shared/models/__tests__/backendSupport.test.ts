import { describe, it, expect } from "vitest";
import { servesModelsByName, SINGLE_MODEL_NOTE } from "../backendSupport";

describe("servesModelsByName", () => {
  it("is true only for Ollama (the multi-model server)", () => {
    expect(servesModelsByName("ollama")).toBe(true);
    expect(servesModelsByName("llama_cpp")).toBe(false);
    expect(servesModelsByName("mlx")).toBe(false);
  });

  it("ships human-readable notes for the single-model limitation", () => {
    expect(SINGLE_MODEL_NOTE).toMatch(/one model at a time/i);
  });
});
