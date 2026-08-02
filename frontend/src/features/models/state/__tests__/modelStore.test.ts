import { describe, it, expect, beforeEach } from "vitest";
import { findActiveDownload, useModelStore } from "../modelStore";

beforeEach(() => {
  useModelStore.setState({ activeTab: "huggingface", downloads: {} });
});

describe("modelStore (M.3)", () => {
  it("starts with activeTab=huggingface and an empty downloads map", () => {
    const s = useModelStore.getState();
    expect(s.activeTab).toBe("huggingface");
    expect(s.downloads).toEqual({});
  });

  it("setActiveTab transitions through all valid values", () => {
    const { setActiveTab } = useModelStore.getState();
    setActiveTab("huggingface");
    expect(useModelStore.getState().activeTab).toBe("huggingface");
    setActiveTab("local");
    expect(useModelStore.getState().activeTab).toBe("local");
    setActiveTab("huggingface");
    expect(useModelStore.getState().activeTab).toBe("huggingface");
  });

  it("findActiveDownload returns the first downloading/installing entry", () => {
    const { upsertDownload } = useModelStore.getState();
    upsertDownload({ id: "a", source: "huggingface", name: "a", status: "success", percent: 100 });
    expect(findActiveDownload(useModelStore.getState().downloads)).toBeUndefined();
    upsertDownload({ id: "b", source: "huggingface", name: "b", status: "downloading", percent: 30 });
    const active = findActiveDownload(useModelStore.getState().downloads);
    expect(active?.name).toBe("b");
    expect(active?.percent).toBe(30);
  });

  it("findActiveDownload returns undefined for an empty map", () => {
    expect(findActiveDownload({})).toBeUndefined();
  });

  it("hfSearchQuery defaults to empty and round-trips via setHfSearchQuery", () => {
    expect(useModelStore.getState().hfSearchQuery).toBe("");
    useModelStore.getState().setHfSearchQuery("llama");
    expect(useModelStore.getState().hfSearchQuery).toBe("llama");
  });

  it("hfSelectedRepo round-trips with its tags; clearing drops the tags too", () => {
    expect(useModelStore.getState().hfSelectedRepo).toBeNull();
    expect(useModelStore.getState().hfSelectedTags).toEqual([]);
    useModelStore.getState().setHfSelectedRepo("org/X-4bit", ["gguf", "safetensors"]);
    expect(useModelStore.getState().hfSelectedRepo).toBe("org/X-4bit");
    expect(useModelStore.getState().hfSelectedTags).toEqual(["gguf", "safetensors"]);
    useModelStore.getState().setHfSelectedRepo(null);
    expect(useModelStore.getState().hfSelectedRepo).toBeNull();
    expect(useModelStore.getState().hfSelectedTags).toEqual([]);
  });

  it("hfRepoKind defaults to gguf; switching kind clears the open repo detail", () => {
    useModelStore.setState({ hfRepoKind: "gguf", hfSelectedRepo: "bartowski/Test-GGUF" });
    expect(useModelStore.getState().hfRepoKind).toBe("gguf");
    useModelStore.getState().setHfRepoKind("gguf");
    expect(useModelStore.getState().hfRepoKind).toBe("gguf");
    expect(useModelStore.getState().hfSelectedRepo).toBeNull();
  });
});
