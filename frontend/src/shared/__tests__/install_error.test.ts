import { describe, it, expect } from "vitest";
import { friendlyInstallError } from "../install_error";

const E = (kind: string, message: string) => ({ kind, message });

describe("friendlyInstallError", () => {
  it("auth required → gated-repo hint", () => {
    expect(friendlyInstallError(E("auth_required", "bartowski/private: HF auth required"))).toMatch(/gated/);
  });

  it("rate limited → wait hint", () => {
    expect(friendlyInstallError(E("inference", "hf search: HF rate limited (HTTP 429)"))).toMatch(/rate-limiting/);
  });

  it("invalid model name → suggest different variant", () => {
    expect(friendlyInstallError(E("validation", "invalid model name"))).toMatch(/name isn't valid/i);
  });

  it("missing file → check the repo and filename", () => {
    expect(friendlyInstallError(E("inference", "file does not exist"))).toMatch(/wasn't found/i);
  });

  it("not_found AppError kind → check tag", () => {
    expect(friendlyInstallError(E("not_found", "model snowflake-arctic-embed:335m"))).toMatch(/wasn't found/);
  });

  it("truncated kind → incomplete-download hint", () => {
    expect(friendlyInstallError(E("truncated", "GGUF truncated: need 8 bytes at offset 8388605, have 3")))
      .toMatch(/incomplete.*download it again|download.*again/i);
  });

  it("big-endian GGUF → format/byte-order hint", () => {
    const e = E("inference", "bad magic: file looks big-endian");
    expect(friendlyInstallError(e)).toMatch(/big-endian|unsupported format/);
  });

  it("bad magic in body → unsupported-format hint", () => {
    const e = E("inference", 'create HTTP 400: {"error":"unsupported architecture: foo"}');
    expect(friendlyInstallError(e)).toMatch(/unsupported|format/i);
  });

  it("timeout kind → network hint", () => {
    expect(friendlyInstallError(E("timeout", "run_prompt timed out after 30000ms"))).toMatch(/timed out.*network/i);
  });

  it("HF HTTP error surfaces the original message", () => {
    expect(friendlyInstallError(E("inference", "hf search: HF HTTP 502"))).toMatch(/Hugging Face.*502/);
  });

  it("accepts a bare string error", () => {
    expect(friendlyInstallError("network is unreachable")).toContain("network is unreachable");
  });

  it("falls back to a generic message when nothing matches and no message present", () => {
    expect(friendlyInstallError({})).toMatch(/unknown reason/);
    expect(friendlyInstallError(null)).toMatch(/unknown reason/);
  });
});
