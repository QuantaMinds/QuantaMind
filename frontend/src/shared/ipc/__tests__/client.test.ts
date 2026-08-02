import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
import { checkLlamaHealth, checkVllmHealth } from "../core/client";

describe("ipc client", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("checkLlamaHealth invokes check_llama_health and returns HealthStatus", async () => {
    vi.mocked(invoke).mockResolvedValue({ available: true, version: null });
    const result = await checkLlamaHealth();
    expect(invoke).toHaveBeenCalledWith("check_llama_health");
    expect(result).toEqual({ available: true, version: null });
  });

  it("checkVllmHealth invokes check_vllm_health and returns HealthStatus", async () => {
    vi.mocked(invoke).mockResolvedValue({ available: false, version: null });
    const result = await checkVllmHealth();
    expect(invoke).toHaveBeenCalledWith("check_vllm_health");
    expect(result).toEqual({ available: false, version: null });
  });

});
