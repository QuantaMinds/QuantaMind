import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue([]) }));
vi.mock("@tauri-apps/plugin-shell", () => ({ open: vi.fn() }));
vi.mock("../../../../shared/ipc/models/ollama_start", () => ({
  startOllama: vi.fn().mockResolvedValue({ status: "started", pid: 1 }),
  stopOllama: vi.fn().mockResolvedValue(undefined),
  isOllamaAutoStartSupported: vi.fn().mockResolvedValue(true),
}));

import { OllamaControl } from "../status/OllamaControl";
import { useBackendStore } from "../../../../shared/state/backendStore";
import { startOllama, stopOllama } from "../../../../shared/ipc/models/ollama_start";

beforeEach(() => {
  vi.clearAllMocks();
  useBackendStore.setState({ ollamaHealthy: null });
});

describe("OllamaControl", () => {
  it("renders nothing while health is unknown", () => {
    const { container } = render(<OllamaControl />);
    expect(container.firstChild).toBeNull();
  });

  it("shows Stop when Ollama is running and stops it on click", async () => {
    useBackendStore.setState({ ollamaHealthy: true });
    render(<OllamaControl />);
    fireEvent.click(screen.getByTestId("ollama-stop"));
    await waitFor(() => expect(stopOllama).toHaveBeenCalled());
    // stopping flips health to false → the control now offers Start
    expect(await screen.findByTestId("ollama-start")).toBeTruthy();
  });

  it("shows Start when Ollama is down and starts it on click", async () => {
    useBackendStore.setState({ ollamaHealthy: false });
    render(<OllamaControl />);
    fireEvent.click(screen.getByTestId("ollama-start"));
    await waitFor(() => expect(startOllama).toHaveBeenCalled());
  });
});
