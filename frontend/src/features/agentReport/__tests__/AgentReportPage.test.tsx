import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

// Spread the real module so the constants/schemas (PASS_K_BY_TIER, TierSchema, …) the
// deep-dive components import survive; mock only the two IPC calls this test drives.
vi.mock("../../../shared/ipc/eval/readiness", async (orig) => ({
  ...(await orig<typeof import("../../../shared/ipc/eval/readiness")>()),
  listReadinessProfiles: vi.fn(),
  assessReadiness: vi.fn(),
}));
vi.mock("../../../shared/ipc/compare/hardware", () => ({
  getHardwareSnapshot: vi.fn(),
  getHardwareTier: vi.fn(),
}));

import { listReadinessProfiles, assessReadiness, type ReadinessProfile } from "../../../shared/ipc/eval/readiness";
import { getHardwareSnapshot } from "../../../shared/ipc/compare/hardware";
import { AgentReportPage } from "../components/AgentReportPage";
import { useEvalRegistryStore } from "../../eval/state/evalRegistryStore";
import { useBatchStore } from "../../eval/state/batchStore";
import { useReadinessStore } from "../state/readinessStore";
import { GIB } from "../capBytes";

const profile = (id: string, name: string, min: number): ReadinessProfile => ({
  id,
  name,
  min_pass_k: min,
  max_avg_steps: null,
  max_ms_per_step: 5000,
  min_context_tokens: null,
  forbid_infinite_loop: true,
  forbid_hallucinated_completion: true,
  require_full_vram: false,
  require_native_fc: false,
});

beforeEach(() => {
  vi.clearAllMocks();
  // Non-empty presets so the page doesn't trigger a registry init() in the test.
  useEvalRegistryStore.setState({ presets: [{ id: "easy-coding", label: "Coding", domain: "coding", tier: "easy", kind: "capability" }], collections: ["finance"], selected: "finance" });
  useReadinessStore.setState({ profiles: [], selectedProfileId: "", verdicts: [], rightSizing: [], rightSizingHint: null, hardware: null, capBytes: null, assessed: false, loading: false, error: null });
  useBatchStore.setState({ report: null });
  vi.mocked(listReadinessProfiles).mockResolvedValue([profile("coding-agent", "Coding agent", 0.8)]);
  vi.mocked(getHardwareSnapshot).mockResolvedValue({
    total_memory_bytes: 64 * GIB,
    available_memory_bytes: 32 * GIB,
    is_apple_silicon: false,
    gpu: { unified: false, available: true, name: "RTX 4090", vram_total_bytes: 24 * GIB },
  });
});

describe("AgentReportPage", () => {
  it("loads profiles and renders the active thresholds readout", async () => {
    render(<AgentReportPage />);
    await waitFor(() => expect(listReadinessProfiles).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByTestId("readiness-thresholds")).toHaveTextContent("Min Pass^k: 80%"));
  });

  it("assesses the selected collection + profile and renders the verdict table", async () => {
    vi.mocked(assessReadiness).mockResolvedValue({
      verdicts: [
        { model: "qwen", backend: "ollama", verdict: { status: "ready", blocking: [], conditions: [], path: "prompt_based" } },
      ],
      right_sizing: [],
      right_sizing_hint: null,
    });
    render(<AgentReportPage />);
    await waitFor(() => expect(screen.getByTestId("readiness-profile-select")).toBeInTheDocument());
    // Wait for the detected hardware cap to populate before running.
    await waitFor(() =>
      expect((screen.getByTestId("readiness-cap-select") as HTMLSelectElement).value).toBe(String(24 * GIB)),
    );
    fireEvent.click(screen.getByTestId("readiness-run"));
    await waitFor(() => expect(assessReadiness).toHaveBeenCalledWith("finance", "coding-agent", 24 * GIB));
    await waitFor(() => expect(screen.getByTestId("readiness-verdict-table")).toBeInTheDocument());
  });

  it("auto-refreshes only when a batch completes for the shown collection or its domain", async () => {
    vi.mocked(assessReadiness).mockResolvedValue({
      verdicts: [
        { model: "qwen", backend: "ollama", verdict: { status: "ready", blocking: [], conditions: [], path: "prompt_based" } },
      ],
      right_sizing: [],
      right_sizing_hint: null,
    });
    // Already assessed for the shown collection (easy-coding); two tier-siblings + a
    // different-domain collection are present so the domain guard can be exercised.
    useEvalRegistryStore.setState({
      presets: [
        { id: "easy-coding", label: "Coding", domain: "coding", tier: "easy", kind: "capability" },
        { id: "medium-coding", label: "Coding", domain: "coding", tier: "medium", kind: "capability" },
        { id: "easy-finance", label: "Finance", domain: "finance", tier: "easy", kind: "capability" },
      ],
      collections: [],
      selected: "easy-coding",
    });
    useReadinessStore.setState({
      profiles: [profile("coding-agent", "Coding agent", 0.8)],
      selectedProfileId: "coding-agent",
      verdicts: [],
      hardware: null,
      capBytes: 1234,
      assessed: true,
      loading: false,
      error: null,
    });
    render(<AgentReportPage />);
    await waitFor(() => expect(listReadinessProfiles).toHaveBeenCalled());
    vi.mocked(assessReadiness).mockClear(); // ignore any mount-time assess

    // A batch in a DIFFERENT domain must be ignored; a same-domain tier sibling triggers
    // exactly one re-assess of the shown collection. (If the different-domain one had
    // fired too, the count would be 2.)
    useBatchStore.setState({ report: { collection_id: "easy-finance", columns: [], num_ctx: null } as never });
    useBatchStore.setState({ report: { collection_id: "medium-coding", columns: [], num_ctx: null } as never });
    await waitFor(() => expect(assessReadiness).toHaveBeenCalledTimes(1));
    expect(assessReadiness).toHaveBeenCalledWith("easy-coding", "coding-agent", 1234);
  });

  it("shows an empty state (not a fabricated verdict) when no report is persisted", async () => {
    vi.mocked(assessReadiness).mockResolvedValue({ verdicts: [], right_sizing: [], right_sizing_hint: null });
    render(<AgentReportPage />);
    await waitFor(() => expect(screen.getByTestId("readiness-profile-select")).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("readiness-run"));
    await waitFor(() => expect(screen.getByTestId("readiness-empty")).toHaveTextContent("No batch report found"));
  });
});
