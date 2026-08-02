import { describe, it, expect } from "vitest";
import { classifyError } from "../core/errorInfo";

describe("classifyError", () => {
  it("classifies a down local server (raw connection error)", () => {
    const info = classifyError("error trying to connect: Connection refused (os error 61)");
    expect(info.title).toMatch(/server isn't running/i);
    expect(info.actionHint).toBe("start_server");
    expect(info.learnMore).toContain("#server-not-running");
  });

  it("classifies a down local server (already-friendly message)", () => {
    const info = classifyError("The local server is not running. Start it and try again.");
    expect(info.title).toMatch(/server isn't running/i);
  });

  it("classifies model-not-found", () => {
    const info = classifyError("model 'llama3' not found");
    expect(info.title).toMatch(/isn't installed/);
    expect(info.actionHint).toBe("open_models");
    expect(info.learnMore).toContain("#model-not-found");
  });

  it("classifies out-of-memory", () => {
    const info = classifyError("llama runner: out of memory");
    expect(info.title).toMatch(/Not enough memory/);
    expect(info.actionHint).toBe("open_models");
  });

  it("classifies timeouts", () => {
    const info = classifyError("request timed out after 60s");
    expect(info.title).toMatch(/timed out/);
    expect(info.actionHint).toBe("retry");
  });

  it("falls back to the raw message for unknown errors", () => {
    const info = classifyError("disk on fire");
    expect(info.title).toBe("Something went wrong");
    expect(info.body).toBe("disk on fire");
    expect(info.learnMore).toBeUndefined();
  });
});
