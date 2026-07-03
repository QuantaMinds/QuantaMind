import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("../../../../shared/ipc/models/llama_start", () => ({
  startLlamaServer: vi.fn(),
  stopLlamaServer: vi.fn(),
}));

import { startLlamaServer } from "../../../../shared/ipc/models/llama_start";
import { LlamaServerControl } from "../status/LlamaServerControl";
import { useBackendStore } from "../../../../shared/state/backendStore";
import { useSelectedModelStore } from "../../../../shared/state/selectedModelStore";

beforeEach(() => {
  vi.clearAllMocks();
  useBackendStore.setState({ llamaHealthy: null });
  useSelectedModelStore.setState({
    selectedModels: [{ name: "phi3", backend: "llama_cpp", size_bytes: 1, path: "/g/phi3.gguf" }],
  });
});

describe("LlamaServerControl", () => {
  it("Start is disabled until a llama.cpp model with a path is selected", () => {
    useSelectedModelStore.setState({ selectedModels: [] });
    render(<LlamaServerControl />);
    expect(screen.getByTestId("llama-start")).toBeDisabled();
  });

  it("surfaces a not_bundled error instead of failing silently", async () => {
    vi.mocked(startLlamaServer).mockResolvedValue({ status: "not_bundled", note: "no binary for this platform" });
    render(<LlamaServerControl />);
    fireEvent.click(screen.getByTestId("llama-start"));
    const err = await screen.findByTestId("llama-start-error");
    expect(err).toHaveTextContent(/no binary/i);
  });

  it("surfaces a start_failed error", async () => {
    vi.mocked(startLlamaServer).mockResolvedValue({ status: "start_failed", error: "port in use" });
    render(<LlamaServerControl />);
    fireEvent.click(screen.getByTestId("llama-start"));
    expect(await screen.findByTestId("llama-start-error")).toHaveTextContent(/port in use/i);
  });

  it("shows Stop once the server is healthy", async () => {
    vi.mocked(startLlamaServer).mockResolvedValue({ status: "started", pid: 1, port: 8080 });
    render(<LlamaServerControl />);
    fireEvent.click(screen.getByTestId("llama-start"));
    await waitFor(() => expect(useBackendStore.getState().llamaHealthy).toBe(true));
  });

  it("shows the hardware-constraint note as a chip with the full text in its popover", async () => {
    vi.mocked(startLlamaServer).mockResolvedValue({
      status: "started",
      pid: 1,
      port: 8080,
      note: "Detected 17 GB of RAM. Running safely: enabled Flash Attention and a Q8 KV cache.",
    });
    render(<LlamaServerControl />);
    fireEvent.click(screen.getByTestId("llama-start"));
    const chip = await screen.findByTestId("llama-start-notice");
    // Compact chip label — the header stays a single clean row…
    expect(chip).toHaveTextContent(/running safely/i);
    // …and the full backend message is present in the popover under the same test id.
    expect(screen.getByTestId("llama-start-notice-popup")).toHaveTextContent(/Flash Attention and a Q8 KV cache/i);
  });

  it("prefers the error chip over the notice when both could apply", async () => {
    vi.mocked(startLlamaServer).mockResolvedValue({ status: "start_failed", error: "port in use" });
    render(<LlamaServerControl />);
    fireEvent.click(screen.getByTestId("llama-start"));
    expect(await screen.findByTestId("llama-start-error")).toHaveTextContent(/start failed/i);
    expect(screen.queryByTestId("llama-start-notice")).toBeNull();
  });
});
