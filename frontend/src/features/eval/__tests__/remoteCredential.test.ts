import { describe, it, expect } from "vitest";
import { remoteCredentialMessage } from "../remoteCredential";
import type { RemoteAuthReport } from "../../../shared/ipc/core/client";

const report = (p: Partial<RemoteAuthReport>): RemoteAuthReport =>
  ({ status: "ok", http_status: null, host: "https://gpu.example.com:8000", insecure_key: false, ...p });

describe("remoteCredentialMessage", () => {
  it("returns null (proceed) only when OK", () => {
    expect(remoteCredentialMessage("vllm", report({ status: "ok" }))).toBeNull();
  });

  it("gives a rejected-key and an unreachable server OPPOSITE, distinct fixes", () => {
    const auth = remoteCredentialMessage("vllm", report({ status: "unauthorized" }))!;
    const down = remoteCredentialMessage("vllm", report({ status: "unreachable" }))!;
    expect(auth).toMatch(/API key/i);
    expect(auth).not.toMatch(/reach|start/i); // never tells you to start a server that's up
    expect(down).toMatch(/reach|start/i);
    expect(down).not.toMatch(/API key/i); // never blames the key for a down server
    expect(auth).not.toEqual(down);
  });

  it("explains a 401 caused by an http key being withheld", () => {
    const msg = remoteCredentialMessage("vllm", report({ status: "unauthorized", insecure_key: true }))!;
    expect(msg).toMatch(/plain http/i);
    expect(msg).toMatch(/https/i);
  });

  it("labels vLLM and covers the rest of the space", () => {
    expect(remoteCredentialMessage("vllm", report({ status: "unconfigured" }))).toMatch(/No vLLM endpoint/);
    expect(remoteCredentialMessage("vllm", report({ status: "tls_error" }))).toMatch(/TLS/);
    expect(remoteCredentialMessage("vllm", report({ status: "not_found" }))).toMatch(/OpenAI-compatible/);
    expect(remoteCredentialMessage("vllm", report({ status: "server_error", http_status: 502 }))).toMatch(/HTTP 502/);
  });

  it("never echoes a key or credentials-in-URL (only the redacted host)", () => {
    // The report only ever carries a redacted host; the message must not add secrets back.
    const msg = remoteCredentialMessage("vllm", report({ status: "unauthorized", host: "https://gpu.example.com:8000" }))!;
    expect(msg).not.toMatch(/sk-|user:|password|@/);
  });
});
