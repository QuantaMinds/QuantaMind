import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { useKvCeilings } from "../useKvCeilings";
import { inspectModel, contextCeilings } from "../../../../shared/ipc/system/inspect";

vi.mock("../../../../shared/ipc/system/inspect", () => ({
  inspectModel: vi.fn(),
  contextCeilings: vi.fn(),
}));

const dims = { layers: 32, head_count: 40, head_count_kv: 8, embedding_length: 5120, context_length: 262_144 };

beforeEach(() => vi.clearAllMocks());

describe("useKvCeilings", () => {
  it("returns ceilings when the IPC chain succeeds", async () => {
    vi.mocked(inspectModel).mockResolvedValue({ dims } as never);
    vi.mocked(contextCeilings).mockResolvedValue({ f16: 14_848, q8: 29_696, q4: 59_648 });
    const { result } = renderHook(() => useKvCeilings("m", "ollama", 9e9, 16e9));
    await waitFor(() => expect(result.current.ceilings).not.toBeNull());
    expect(result.current.ceilings).toEqual({ f16: 14_848, q8: 29_696, q4: 59_648 });
  });

  it("degrades to null (not zeros, not stale) when inspectModel rejects — backend unreachable", async () => {
    vi.mocked(inspectModel).mockRejectedValue(new Error("connection refused"));
    const { result } = renderHook(() => useKvCeilings("m", "ollama", 9e9, 16e9));
    await waitFor(() => expect(inspectModel).toHaveBeenCalled());
    expect(result.current.dims).toBeNull();
    expect(result.current.ceilings).toBeNull();
  });

  it("degrades to null when the ceilings IPC itself rejects", async () => {
    vi.mocked(inspectModel).mockResolvedValue({ dims } as never);
    vi.mocked(contextCeilings).mockRejectedValue(new Error("ipc error"));
    const { result } = renderHook(() => useKvCeilings("m", "ollama", 9e9, 16e9));
    await waitFor(() => expect(contextCeilings).toHaveBeenCalled());
    expect(result.current.ceilings).toBeNull();
  });

  it("does not call the ceilings IPC without weights or total memory", async () => {
    vi.mocked(inspectModel).mockResolvedValue({ dims } as never);
    renderHook(() => useKvCeilings("m", "ollama", null, 16e9));
    await waitFor(() => expect(inspectModel).toHaveBeenCalled());
    expect(contextCeilings).not.toHaveBeenCalled();
  });
});
