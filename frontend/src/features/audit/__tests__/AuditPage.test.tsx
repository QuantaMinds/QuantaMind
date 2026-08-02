import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue([]) }));

/// `load_collection_history` returns { entries, unreadable } — the readable rows
/// plus a count of the stored rows this build couldn't interpret.
const hist = (entries: unknown[], unreadable = 0) => Promise.resolve({ entries, unreadable });
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

const summary = (model: string, backend: "llama_cpp" | "vllm") => ({
  ts: "2026-06-01T00:00:00Z", model, backend,
  parse_rate: null, tool_selection_acc: null, arg_acc: null, abstain_acc: null,
  composite: 0.8, n: 5,
});

beforeEach(() => {
  vi.clearAllMocks();
  useBackendStore.setState({ selectedBackend: "llama_cpp" });
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
        return hist([summary("llama3", "vllm"), summary("qwen.gguf", "llama_cpp")]);
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
      cmd === "load_collection_history" ? hist([summary("qwen.gguf", "llama_cpp")]) : Promise.resolve([]),
    );
    useBackendStore.setState({ selectedBackend: "vllm" }); // no vLLM runs, one llama.cpp run
    render(<AuditPage />);
    const note = await screen.findByTestId("audit-history-other-backend");
    expect(note).toHaveTextContent("No runs for vLLM yet");
    expect(note).toHaveTextContent("1 run recorded under other backends");
    expect(screen.queryByTestId("history-timeline")).not.toBeInTheDocument();
  });

  it("re-fetches history live only when a batch completes for the shown collection", async () => {
    let n = 0; // count of load_collection_history calls
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === "load_collection_history") {
        n += 1;
        return hist(n === 1 ? [] : [summary("llama3", "llama_cpp")]);
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

/// Regression: a run recorded on a now-removed backend used to fail the WHOLE
/// read, so one legacy row blanked the panel with "Couldn't load run history —
/// unknown variant `…`". The readable runs must render, and what was skipped
/// must be stated — a short list can't be allowed to read as the full record.
it("renders the readable runs and NAMES the skipped ones when history has legacy rows", async () => {
  vi.mocked(invoke).mockImplementation((cmd: string) =>
    cmd === "load_collection_history" ? hist([summary("qwen.gguf", "llama_cpp")], 3) : Promise.resolve([]),
  );
  render(<AuditPage />);

  // The graph renders — no error state, no blanked panel.
  await waitFor(() => expect(screen.getByTestId("history-timeline")).toHaveTextContent("qwen.gguf"));
  expect(screen.queryByTestId("audit-history-error")).not.toBeInTheDocument();

  // …and the skipped rows are stated, not silently dropped.
  const note = screen.getByTestId("audit-history-unreadable");
  expect(note).toHaveTextContent("3 older runs couldn't be read");
  expect(note).toHaveTextContent("still on disk, untouched");
});

it("says nothing about skipped rows when every row was readable", async () => {
  vi.mocked(invoke).mockImplementation((cmd: string) =>
    cmd === "load_collection_history" ? hist([summary("qwen.gguf", "llama_cpp")], 0) : Promise.resolve([]),
  );
  render(<AuditPage />);
  await waitFor(() => expect(screen.getByTestId("history-timeline")).toBeInTheDocument());
  expect(screen.queryByTestId("audit-history-unreadable")).not.toBeInTheDocument();
});
