import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../../shared/ipc/eval/registry", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  getBuiltinCollection: vi.fn().mockResolvedValue([]),
  listBuiltinCollections: vi.fn().mockResolvedValue([]),
  listCustomCollections: vi.fn().mockResolvedValue(["fresh"]),
  loadCustomCollection: vi.fn().mockResolvedValue([]),
  saveCustomCollection: vi.fn().mockResolvedValue(undefined),
  deleteCustomCollection: vi.fn().mockResolvedValue(undefined),
  importCustomCollection: vi.fn().mockResolvedValue("fresh"),
  validateCollectionFile: vi.fn(),
  validateCustomCollection: vi.fn(),
}));

import { useEvalRegistryStore } from "../state/evalRegistryStore";
import {
  importCustomCollection,
  validateCollectionFile,
  validateCustomCollection,
  type ToolTask,
  type CollectionValidation,
} from "../../../shared/ipc/eval/registry";

const task: ToolTask = {
  id: "t1",
  category: "agent_loop",
  prompt: "p",
  tools: [{ name: "x", description: "", parameters: { type: "object", properties: {} } }],
  expected: { type: "no_call" },
  agentic: { mocks: [], end_state: { require_end_state: {} }, world_state: { a: "1" } },
};

const failingVerdict: CollectionValidation = {
  ok: false,
  structural_error: null,
  tasks: [{ id: "t1", reachable: "yes", discriminating: true, detail: "d", semantic: ["t1: world_state entity 'Z-99' is orphaned"], semantic_warnings: []  }],
};
const cleanVerdict: CollectionValidation = {
  ok: true,
  structural_error: null,
  tasks: [{ id: "t1", reachable: "yes", discriminating: true, detail: "d", semantic: [], semantic_warnings: []  }],
};

beforeEach(() => {
  vi.clearAllMocks();
});

describe("evalRegistryStore.editWorldState", () => {
  it("replaces the task's world_state and marks the selection edited (fork-on-edit)", () => {
    useEvalRegistryStore.setState({ tasks: [task], edited: false });
    useEvalRegistryStore.getState().editWorldState("t1", { a: "2", b: "3" });
    const s = useEvalRegistryStore.getState();
    expect(s.edited).toBe(true);
    expect(s.tasks[0].agentic?.world_state).toEqual({ a: "2", b: "3" });
  });

  it("leaves other tasks untouched", () => {
    const other: ToolTask = { ...task, id: "t2", agentic: { ...task.agentic!, world_state: { keep: "me" } } };
    useEvalRegistryStore.setState({ tasks: [task, other], edited: false });
    useEvalRegistryStore.getState().editWorldState("t1", { a: "9" });
    expect(useEvalRegistryStore.getState().tasks[1].agentic?.world_state).toEqual({ keep: "me" });
  });
});

const warnedVerdict: CollectionValidation = {
  ok: true,
  structural_error: null,
  tasks: [{ id: "t1", reachable: "yes", discriminating: true, detail: "d", semantic: [], semantic_warnings: ["t1: expected log({...}) globs on 'quantize' — checked the prompt..."] }],
};

describe("evalRegistryStore.importFile (two-phase: validate the FILE before writing)", () => {
  it("imports an ok-with-warnings file AND returns the verdict so warnings surface", async () => {
    vi.mocked(validateCollectionFile).mockResolvedValue(warnedVerdict);
    useEvalRegistryStore.setState({ collections: [], presets: [] });
    const verdict = await useEvalRegistryStore.getState().importFile("/tmp/warned.json");
    expect(importCustomCollection).toHaveBeenCalledWith("/tmp/warned.json"); // NOT blocked
    expect(verdict).toEqual(warnedVerdict); // but advisories returned for display
  });

  it("returns the failing verdict and imports NOTHING when the dry-run fails", async () => {
    vi.mocked(validateCollectionFile).mockResolvedValue(failingVerdict);
    useEvalRegistryStore.setState({ collections: [], presets: [] });
    const verdict = await useEvalRegistryStore.getState().importFile("/tmp/broken.json");
    expect(verdict).toEqual(failingVerdict);
    expect(importCustomCollection).not.toHaveBeenCalled();
    expect(useEvalRegistryStore.getState().collections).toEqual([]);
  });

  it("imports and selects the collection when the dry-run is clean, returning null", async () => {
    vi.mocked(validateCollectionFile).mockResolvedValue(cleanVerdict);
    useEvalRegistryStore.setState({ collections: [], presets: [] });
    const verdict = await useEvalRegistryStore.getState().importFile("/tmp/good.json");
    expect(verdict).toBeNull();
    expect(importCustomCollection).toHaveBeenCalledWith("/tmp/good.json");
    const s = useEvalRegistryStore.getState();
    expect(s.collections).toEqual(["fresh"]);
    expect(s.selected).toBe("fresh");
  });
});

describe("evalRegistryStore.save (auto-validate after write)", () => {
  it("returns the fresh verdict for the saved collection", async () => {
    vi.mocked(validateCustomCollection).mockResolvedValue(cleanVerdict);
    useEvalRegistryStore.setState({ presets: [] });
    const verdict = await useEvalRegistryStore.getState().save("mine", [task]);
    expect(validateCustomCollection).toHaveBeenCalledWith("mine");
    expect(verdict).toEqual(cleanVerdict);
  });

  it("still saves (returns null) when the auto-validate call itself hiccups", async () => {
    vi.mocked(validateCustomCollection).mockRejectedValue(new Error("ipc down"));
    useEvalRegistryStore.setState({ presets: [] });
    const verdict = await useEvalRegistryStore.getState().save("mine", [task]);
    expect(verdict).toBeNull();
    expect(useEvalRegistryStore.getState().selected).toBe("mine");
  });
});
