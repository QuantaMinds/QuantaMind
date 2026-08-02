import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook } from "@testing-library/react";

const onDragDropEvent = vi.fn();
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent }),
}));

import { useModalDragDrop } from "../useModalDragDrop";
import { useModelStore } from "../../state/modelStore";

beforeEach(() => {
  onDragDropEvent.mockReset();
  useModelStore.setState({
    activeTab: "huggingface", pendingLocalPath: null,
    downloads: {}, activeHfName: null, activeLocalName: null,
  });
});

async function captureHandler() {
  let captured: ((e: unknown) => void) | null = null;
  onDragDropEvent.mockImplementation(async (h: typeof captured) => {
    captured = h;
    return () => {};
  });
  renderHook(() => useModalDragDrop(true));
  await new Promise((r) => setTimeout(r, 0));
  return captured!;
}

describe("useModalDragDrop", () => {
  it("does not attach a listener when inactive", () => {
    renderHook(() => useModalDragDrop(false));
    expect(onDragDropEvent).not.toHaveBeenCalled();
  });

  it("imports the .gguf path on drop and switches to the local tab", async () => {
    const handler = await captureHandler();
    handler({ payload: { type: "drop", paths: ["/foo/model.gguf"] } });
    expect(useModelStore.getState().pendingLocalPath).toBe("/foo/model.gguf");
    expect(useModelStore.getState().activeTab).toBe("local");
  });

  it("ignores drops with no .gguf and logs a warning", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const handler = await captureHandler();
    handler({ payload: { type: "drop", paths: ["/foo/image.png"] } });
    expect(useModelStore.getState().pendingLocalPath).toBeNull();
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });

  it("takes the first of multiple .gguf and logs about the extras", async () => {
    const info = vi.spyOn(console, "info").mockImplementation(() => {});
    const handler = await captureHandler();
    handler({ payload: { type: "drop", paths: ["/a.gguf", "/b.gguf", "/c.gguf"] } });
    expect(useModelStore.getState().pendingLocalPath).toBe("/a.gguf");
    expect(info).toHaveBeenCalled();
    info.mockRestore();
  });

  it("non-drop events are ignored (hover, enter, leave)", async () => {
    const handler = await captureHandler();
    handler({ payload: { type: "enter", paths: ["/x.gguf"] } });
    expect(useModelStore.getState().pendingLocalPath).toBeNull();
  });
});
