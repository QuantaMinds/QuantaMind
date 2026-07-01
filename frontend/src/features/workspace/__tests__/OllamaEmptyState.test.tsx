import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-shell", () => ({ open: vi.fn().mockResolvedValue(undefined) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));

import { invoke } from "@tauri-apps/api/core";
import { open as openExternal } from "@tauri-apps/plugin-shell";
import { OllamaEmptyState } from "../components/status/OllamaEmptyState";
import { useBackendStore } from "../../../shared/state/backendStore";
import { useInstalledModelsStore } from "../../models/state/installedModelsStore";

// Every test renders <OllamaEmptyState/>, which fires an `ollama_auto_start_supported`
// probe on mount in addition to whatever `start_ollama` call a click triggers — mock
// by command name so that mount-time probe never consumes a `start_ollama`-only
// sequenced response.
function mockInvoke(startOllamaResponses: unknown[]) {
  let call = 0;
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    if (cmd === "ollama_auto_start_supported") return Promise.resolve(true);
    const response = startOllamaResponses[Math.min(call, startOllamaResponses.length - 1)];
    call += 1;
    return Promise.resolve(response);
  });
}

beforeEach(() => {
  vi.mocked(invoke).mockReset();
  mockInvoke([]);
  vi.mocked(openExternal).mockReset().mockResolvedValue(undefined);
  useBackendStore.setState({ ollamaHealthy: false });
  useInstalledModelsStore.setState({
    list: [], status: "idle", error: null, lastRefreshedAt: null,
  });
});

describe("OllamaEmptyState", () => {
  it("idle state renders both Start and Install actions", () => {
    render(<OllamaEmptyState />);
    expect(screen.getByText(/Ollama is not running/)).toBeInTheDocument();
    expect(screen.getByTestId("ollama-start-button")).toBeInTheDocument();
    expect(screen.getByTestId("ollama-install-link")).toBeInTheDocument();
  });

  it("clicking Start moves to starting → success and flips ollamaHealthy true", async () => {
    vi.useFakeTimers();
    mockInvoke([{ status: "started", pid: 4242 }]);
    render(<OllamaEmptyState />);
    fireEvent.click(screen.getByTestId("ollama-start-button"));
    await vi.waitFor(() =>
      expect(screen.getByText(/Ollama started/)).toBeInTheDocument(),
    );
    vi.advanceTimersByTime(1100);
    await vi.waitFor(() =>
      expect(useBackendStore.getState().ollamaHealthy).toBe(true),
    );
    vi.useRealTimers();
  });

  it("not_installed shows install button that calls shell.open", async () => {
    mockInvoke([{ status: "not_installed", install_url: "https://ollama.com/download" }]);
    render(<OllamaEmptyState />);
    fireEvent.click(screen.getByTestId("ollama-start-button"));
    await waitFor(() => expect(screen.getByTestId("ollama-install-button")).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("ollama-install-button"));
    await waitFor(() =>
      expect(openExternal).toHaveBeenCalledWith("https://ollama.com/download"),
    );
  });

  it("start_failed shows error message and Retry triggers another start", async () => {
    mockInvoke([
      { status: "start_failed", error: "Port 11434 in use" },
      { status: "start_failed", error: "still in use" },
    ]);
    render(<OllamaEmptyState />);
    fireEvent.click(screen.getByTestId("ollama-start-button"));
    await waitFor(() =>
      expect(screen.getByTestId("ollama-error-message")).toHaveTextContent(/Port 11434 in use/),
    );
    fireEvent.click(screen.getByTestId("ollama-retry-button"));
    await waitFor(() =>
      expect(screen.getByTestId("ollama-error-message")).toHaveTextContent(/still in use/),
    );
    const startCalls = vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === "start_ollama");
    expect(startCalls).toHaveLength(2);
  });

  it("install link in idle state also opens the official download page", async () => {
    render(<OllamaEmptyState />);
    fireEvent.click(screen.getByTestId("ollama-install-link"));
    await waitFor(() =>
      expect(openExternal).toHaveBeenCalledWith("https://ollama.com/download"),
    );
  });

  // Phase 2 runtime-contract update: when `ollama_auto_start_supported()`
  // returns false, the meaning is "not found on disk" (was: "wrong OS"). The
  // copy shifts from "Auto-start isn't supported on this OS yet" to
  // "Ollama isn't installed on this machine" with a per-OS install command
  // block. Test asserts the new copy shape.
  it("when Ollama isn't on disk, empty state offers a per-OS install command and Check again", async () => {
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === "ollama_auto_start_supported") return Promise.resolve(false);
      if (cmd === "os_platform") return Promise.resolve("macos");
      return Promise.resolve(undefined);
    });
    render(<OllamaEmptyState />);
    await waitFor(() =>
      expect(screen.getByTestId("ollama-check-again-button")).toBeInTheDocument(),
    );
    expect(screen.queryByTestId("ollama-start-button")).not.toBeInTheDocument();
    expect(screen.getByText(/Ollama isn't installed on this machine/)).toBeInTheDocument();
    // The per-OS install command block is present with a Copy button.
    expect(screen.getByTestId("ollama-install-cmd")).toBeInTheDocument();
    expect(screen.getByTestId("ollama-install-copy")).toBeInTheDocument();
  });

  // The `manual_start_required` variant is unreachable from Rust as of Phase 2
  // (see `OllamaStartResult` docstring), but the frontend still renders it if
  // it ever surfaces. Test kept for wire-format compat.
  it("manual_start_required shows manual-start guidance with a Check again action", async () => {
    mockInvoke([{ status: "manual_start_required", install_url: "https://ollama.com/download" }]);
    render(<OllamaEmptyState />);
    fireEvent.click(screen.getByTestId("ollama-start-button"));
    await waitFor(() =>
      expect(screen.getByText(/needs to be started manually/)).toBeInTheDocument(),
    );
    expect(screen.getByTestId("ollama-check-again-button")).toBeInTheDocument();
  });
});
