import { describe, expect, it, vi, beforeEach } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import { invoke } from "@tauri-apps/api/core";
import { startLlamaServer, stopLlamaServer, listLlamaModels } from "../llama_start";

beforeEach(() => vi.mocked(invoke).mockReset());

describe("llama_start IPC wrappers", () => {
  it("startLlamaServer passes the model path (numCtx null when unset) and parses a started result", async () => {
    vi.mocked(invoke).mockResolvedValue({ status: "started", pid: 7, port: 8080 });
    const r = await startLlamaServer("/g/phi3.gguf");
    expect(invoke).toHaveBeenCalledWith("start_llama_server", { modelPath: "/g/phi3.gguf", numCtx: null });
    expect(r).toEqual({ status: "started", pid: 7, port: 8080 });
  });

  it("startLlamaServer forwards the Context window param as numCtx", async () => {
    vi.mocked(invoke).mockResolvedValue({ status: "started", pid: 1, port: 8081 });
    await startLlamaServer("/g/phi3.gguf", 16384);
    expect(invoke).toHaveBeenCalledWith("start_llama_server", { modelPath: "/g/phi3.gguf", numCtx: 16384 });
  });

  it("startLlamaServer parses the not_bundled and start_failed variants", async () => {
    vi.mocked(invoke).mockResolvedValue({ status: "not_bundled", note: "no binary" });
    expect(await startLlamaServer("/g/x.gguf")).toEqual({ status: "not_bundled", note: "no binary" });
  });

  it("startLlamaServer rejects an unknown status", async () => {
    vi.mocked(invoke).mockResolvedValue({ status: "exploded" });
    await expect(startLlamaServer("/g/x.gguf")).rejects.toBeTruthy();
  });

  it("listLlamaModels parses GGUF models tagged llama_cpp with a path", async () => {
    vi.mocked(invoke).mockResolvedValue([
      { name: "phi3", size_bytes: 100, modified_at: "", family: "phi", parameter_size: "",
        quantization: "Q4_K_M", backend: "llama_cpp", path: "/g/phi3.gguf" },
    ]);
    const models = await listLlamaModels();
    expect(models[0].backend).toBe("llama_cpp");
    expect(models[0].path).toBe("/g/phi3.gguf");
  });

  it("stopLlamaServer invokes the stop command", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);
    await stopLlamaServer();
    expect(invoke).toHaveBeenCalledWith("stop_llama_server");
  });
});
