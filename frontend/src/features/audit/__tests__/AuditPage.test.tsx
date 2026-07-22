import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue([]) }));
// The probe has its own suite; stub it. The timeline renders the models it got so
// we can assert the backend filter.
vi.mock("../../eval/components/ContextCliffPanel", () => ({ ContextCliffPanel: () => <div data-testid="cliff-panel" /> }));
vi.mock("../../eval/components/matrix/HistoryTimeline", () => ({
  HistoryTimeline: ({ history }: { history: { model: string }[] }) => (
    <div data-testid="history-timeline">{history.map((h) => h.model).join(",")}</div>
  ),
}));

import { invoke } from "@tauri-apps/api/core";
import { AuditPage } from "../components/AuditPage";
import { useEvalRegistryStore } from "../../eval/state/evalRegistryStore";
import { useBatchStore } from "../../eval/state/batchStore";
import { useBackendStore } from "../../../shared/state/backendStore";

const summary = (model: string, backend: "ollama" | "llama_cpp") => ({
  ts: "2026-06-01T00:00:00Z", model, backend,
  parse_rate: null, tool_selection_acc: null, arg_acc: null, abstain_acc: null,
  composite: 0.8, n: 5,
});

beforeEach(() => {
  vi.clearAllMocks();
  useBackendStore.setState({ selectedBackend: "ollama" });
  useBatchStore.setState({ report: null });
  useEvalRegistryStore.setState({ presets: [{ id: "easy-coding", label: "Coding", domain: "coding", tier: "easy" }], collections: [], init: vi.fn().mockResolvedValue(undefined) });
});

describe("AuditPage", () => {
  it("mounts the saved-history section, the export, and the Context-Cliff probe", async () => {
    render(<AuditPage />);
    expect(screen.getByTestId("tab-audit")).toBeInTheDocument();
    expect(screen.getByTestId("audit-history")).toBeInTheDocument();
    expect(screen.getByTestId("audit-export-csv")).toBeDisabled(); // no run yet
    expect(screen.getByTestId("cliff-panel")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByTestId("history-timeline")).toBeInTheDocument());
  });

  it("shows only the selected backend's models in the history (not the previous backend's)", async () => {
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === "load_collection_history")
        return Promise.resolve([summary("llama3", "ollama"), summary("qwen.gguf", "llama_cpp")]);
      return Promise.resolve([]);
    });
    useBackendStore.setState({ selectedBackend: "llama_cpp" });
    render(<AuditPage />);
    await waitFor(() => expect(screen.getByTestId("history-timeline")).toHaveTextContent("qwen.gguf"));
    expect(screen.getByTestId("history-timeline")).not.toHaveTextContent("llama3");
  });

  it("tier-groups the collection picker so duplicate domain labels are distinguishable", async () => {
    // The backend humanizes labels by stripping the tier prefix, so easy/medium/hard-coding
    // all read "Coding". A flat list showed three indistinguishable options; PresetOptGroups
    // separates them under Easy/Medium/Hard <optgroup>s.
    useEvalRegistryStore.setState({
      presets: [
        { id: "easy-coding", label: "Coding", domain: "coding", tier: "easy" },
        { id: "medium-coding", label: "Coding", domain: "coding", tier: "medium" },
        { id: "hard-coding", label: "Coding", domain: "coding", tier: "hard" },
      ],
    });
    render(<AuditPage />);
    const select = screen.getByTestId("audit-collection") as HTMLSelectElement;
    const groups = Array.from(select.querySelectorAll("optgroup")).map((g) => g.label);
    expect(groups).toEqual(["Easy", "Medium", "Hard"]); // one group per tier, ordered
    // Each duplicate "Coding" lives under its own tier group → its value is unique.
    select.querySelectorAll("optgroup").forEach((g) => {
      const opts = Array.from(g.querySelectorAll("option"));
      expect(opts).toHaveLength(1);
      expect(opts[0].textContent).toBe("Coding");
    });
    expect(Array.from(select.querySelectorAll("option")).map((o) => o.value)).toEqual([
      "easy-coding", "medium-coding", "hard-coding",
    ]);
  });

  it("surfaces a load failure instead of a misleading 'no runs yet' empty state", async () => {
    vi.mocked(invoke).mockImplementation((cmd: string) =>
      cmd === "load_collection_history" ? Promise.reject("disk boom") : Promise.resolve([]),
    );
    render(<AuditPage />);
    await waitFor(() => expect(screen.getByTestId("audit-history-error")).toHaveTextContent("disk boom"));
    // The misleading empty chart must NOT also render — a failure is not "no runs yet".
    expect(screen.queryByTestId("history-timeline")).not.toBeInTheDocument();
  });

  it("tells the user runs exist under another backend rather than 'no runs yet'", async () => {
    vi.mocked(invoke).mockImplementation((cmd: string) =>
      cmd === "load_collection_history" ? Promise.resolve([summary("qwen.gguf", "llama_cpp")]) : Promise.resolve([]),
    );
    useBackendStore.setState({ selectedBackend: "ollama" }); // no ollama runs, one llama.cpp run
    render(<AuditPage />);
    const note = await screen.findByTestId("audit-history-other-backend");
    expect(note).toHaveTextContent("No runs for Ollama yet");
    expect(note).toHaveTextContent("1 run recorded under other backends");
    expect(screen.queryByTestId("history-timeline")).not.toBeInTheDocument();
  });

  it("re-fetches history live only when a batch completes for the shown collection", async () => {
    let n = 0; // count of load_collection_history calls
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === "load_collection_history") {
        n += 1;
        return Promise.resolve(n === 1 ? [] : [summary("llama3", "ollama")]);
      }
      return Promise.resolve([]);
    });
    render(<AuditPage />);
    await waitFor(() => expect(n).toBe(1)); // initial load on mount

    // A run for a DIFFERENT collection must be ignored; the matching one re-fetches.
    // If the non-match had triggered, n would reach 3 — asserting n===2 proves it didn't.
    useBatchStore.setState({ report: { collection_id: "other" } as never });
    useBatchStore.setState({ report: { collection_id: "easy-coding" } as never });
    await waitFor(() => expect(screen.getByTestId("history-timeline")).toHaveTextContent("llama3"));
    expect(n).toBe(2);
  });
});
