import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { createRef } from "react";

vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));
vi.mock("../../../shared/ipc/publish/export", () => ({ saveReadinessImage: vi.fn() }));
vi.mock("../export/snapshot", () => ({ snapshotPng: vi.fn() }));

import { save } from "@tauri-apps/plugin-dialog";
import { saveReadinessImage } from "../../../shared/ipc/publish/export";
import { snapshotPng } from "../export/snapshot";
import { ExportMenu } from "../components/ExportMenu";
import { ToastHost } from "../../../shared/ui/Toast";
import type { ModelVerdict, ReadinessProfile } from "../../../shared/ipc/eval/readiness";

const PROFILE: ReadinessProfile = {
  id: "coding", name: "Coding agent", min_pass_k: 0.8, max_avg_steps: null, max_ms_per_step: null,
  min_context_tokens: null, forbid_infinite_loop: true, forbid_hallucinated_completion: true,
  require_full_vram: false, require_native_fc: false,
};
const VERDICTS: ModelVerdict[] = [
  { model: "qwen", backend: "llama_cpp", verdict: { status: "ready", blocking: [], conditions: [], path: "native_fc" }, pass_k: 0.9 },
];

function renderMenu() {
  const cardRef = createRef<HTMLDivElement>();
  (cardRef as { current: HTMLDivElement | null }).current = document.createElement("div");
  render(
    <>
      <ExportMenu verdicts={VERDICTS} profile={PROFILE} collectionId="finance" hardware={null} cardRef={cardRef} />
      <ToastHost />
    </>,
  );
  fireEvent.click(screen.getByTestId("readiness-export"));
}

const setClipboard = (writeText: () => Promise<void>) =>
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });

describe("ExportMenu", () => {
  beforeEach(() => vi.clearAllMocks());

  it("opens to the three offline export options", () => {
    renderMenu();
    expect(screen.getByTestId("export-image")).toBeInTheDocument();
    expect(screen.getByTestId("export-markdown")).toBeInTheDocument();
    expect(screen.getByTestId("export-html")).toBeInTheDocument();
  });

  it("copies the built Markdown to the clipboard and confirms via toast", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    setClipboard(writeText);
    renderMenu();
    fireEvent.click(screen.getByTestId("export-markdown"));
    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
    expect(writeText.mock.calls[0][0]).toContain("# Local Agent Readiness — finance");
    await waitFor(() => expect(screen.getByTestId("toast")).toHaveTextContent("Markdown copied"));
  });

  it("surfaces a focus-rejection instead of silently failing", async () => {
    setClipboard(vi.fn().mockRejectedValue(new Error("Document is not focused")));
    renderMenu();
    fireEvent.click(screen.getByTestId("export-markdown"));
    await waitFor(() => expect(screen.getByTestId("toast")).toHaveTextContent("ensure the app has focus"));
  });

  it("snapshots the card, prompts for a path, and writes the PNG bytes", async () => {
    vi.mocked(snapshotPng).mockResolvedValue(new Uint8Array([1, 2, 3]));
    vi.mocked(save).mockResolvedValue("/tmp/card.png");
    renderMenu();
    fireEvent.click(screen.getByTestId("export-image"));
    await waitFor(() => expect(saveReadinessImage).toHaveBeenCalledWith("/tmp/card.png", new Uint8Array([1, 2, 3])));
    expect(snapshotPng).toHaveBeenCalledTimes(1);
  });

  it("does not write when the user cancels the save dialog", async () => {
    vi.mocked(snapshotPng).mockResolvedValue(new Uint8Array([1]));
    vi.mocked(save).mockResolvedValue(null);
    renderMenu();
    fireEvent.click(screen.getByTestId("export-image"));
    await waitFor(() => expect(snapshotPng).toHaveBeenCalled());
    expect(saveReadinessImage).not.toHaveBeenCalled();
  });
});
