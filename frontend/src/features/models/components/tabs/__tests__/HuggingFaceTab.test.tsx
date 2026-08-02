import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor, act } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));

import { invoke } from "@tauri-apps/api/core";
import { HuggingFaceTab } from "../HuggingFaceTab";
import { useModelStore } from "../../../state/modelStore";

const HIT = (id: string, downloads = 100) => ({
  id, downloads, likes: 1, tags: ["gguf"], last_modified: null,
});

beforeEach(() => {
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    if (cmd === "hf_search")
      return Promise.resolve([HIT("bartowski/Llama-GGUF", 1234), HIT("other/Repo-GGUF", 42)]);
    if (cmd === "hf_repo_files") return Promise.resolve([]);
    if (cmd === "hf_model_card") return Promise.resolve(null);
    if (cmd === "list_models") return Promise.resolve([]);
    return Promise.reject(new Error(`unknown ${cmd}`));
  });
  // Reset the persisted HF state so tests don't leak query/selected across runs.
  useModelStore.setState({ hfSearchQuery: "", hfSelectedRepo: null, hfRepoKind: "gguf" });
});

describe("HuggingFaceTab (live search)", () => {
  it("starts in the idle state with a hint and no cards", () => {
    render(<HuggingFaceTab />);
    expect(screen.getByTestId("hf-idle")).toBeInTheDocument();
    expect(screen.queryByTestId("hf-card-bartowski/Llama-GGUF")).toBeNull();
  });

  it("typing triggers an hf_search and renders result cards", async () => {
    render(<HuggingFaceTab />);
    fireEvent.change(screen.getByLabelText("Search Hugging Face"), { target: { value: "llama" } });
    await waitFor(
      () => expect(invoke).toHaveBeenCalledWith("hf_search", { query: "llama", limit: 30, kind: "gguf" }),
      { timeout: 1000 },
    );
    await screen.findByTestId("hf-card-bartowski/Llama-GGUF");
    expect(screen.getByTestId("hf-card-other/Repo-GGUF")).toBeInTheDocument();
  });

  it("renders an error state when hf_search rejects", async () => {
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === "hf_search") return Promise.reject({ kind: "inference", message: "HF down" });
      return Promise.resolve([]);
    });
    render(<HuggingFaceTab />);
    fireEvent.change(screen.getByLabelText("Search Hugging Face"), { target: { value: "x" } });
    await screen.findByTestId("hf-error-search", undefined, { timeout: 1000 });
    expect(screen.getByTestId("hf-error-search")).toHaveTextContent(/HF down/);
  });

  it("clicking a card opens the repo detail with the right repo", async () => {
    render(<HuggingFaceTab />);
    fireEvent.change(screen.getByLabelText("Search Hugging Face"), { target: { value: "llama" } });
    const card = await screen.findByTestId("hf-card-bartowski/Llama-GGUF", undefined, { timeout: 1000 });
    fireEvent.click(card);
    expect(screen.getByTestId("hf-repo-detail")).toBeInTheDocument();
    expect(screen.getByTestId("hf-repo-detail")).toHaveTextContent("bartowski/Llama-GGUF");
    expect(useModelStore.getState().hfSelectedRepo).toBe("bartowski/Llama-GGUF");
  });

  it("the search query survives a select-then-back round-trip", async () => {
    render(<HuggingFaceTab />);
    fireEvent.change(screen.getByLabelText("Search Hugging Face"), { target: { value: "llama" } });
    const card = await screen.findByTestId("hf-card-bartowski/Llama-GGUF", undefined, { timeout: 1000 });
    fireEvent.click(card);
    // Programmatically clear hfSelectedRepo (the back button's effect).
    await act(async () => useModelStore.getState().setHfSelectedRepo(null));
    expect(useModelStore.getState().hfSearchQuery).toBe("llama");
    // After back the search input shows the query and the cards re-render.
    expect((screen.getByLabelText("Search Hugging Face") as HTMLInputElement).value).toBe("llama");
    expect(await screen.findByTestId("hf-card-bartowski/Llama-GGUF")).toBeInTheDocument();
  });
});
