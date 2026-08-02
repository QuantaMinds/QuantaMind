import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, render } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../../../../shared/ipc/models/hf_install", () => ({
  cancelHfInstall: vi.fn(),
}));

import { DownloadsActive } from "../DownloadsActive";
import { useModelStore } from "../../../state/modelStore";

beforeEach(() => {
  useModelStore.setState({ downloads: {} });
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("DownloadsActive auto-clear", () => {
  it("auto-clears success entries after 5s; keeps error entries pending dismissal", () => {
    useModelStore.getState().upsertDownload({
      id: "good",
      source: "huggingface",
      name: "good",
      status: "success",
      percent: 100,
    });
    useModelStore.getState().upsertDownload({
      id: "bad",
      source: "huggingface",
      name: "bad",
      status: "error",
      percent: 0,
      error: "x",
    });
    render(<DownloadsActive />);
    expect(useModelStore.getState().downloads["good"]).toBeDefined();
    act(() => {
      vi.advanceTimersByTime(5000);
    });
    expect(useModelStore.getState().downloads["good"]).toBeUndefined();
    expect(useModelStore.getState().downloads["bad"]).toBeDefined();
  });
});
