import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue(undefined) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));

import { invoke } from "@tauri-apps/api/core";
import { SingleRun } from "../run/SingleRun";
import { useBackendStore } from "../../../../shared/state/backendStore";
import { useParamsStore } from "../../../../shared/state/paramsStore";
import { useWorkspacesStore } from "../../../workspaces/state/workspaceStore";

beforeEach(() => {
  vi.clearAllMocks();
  useBackendStore.setState({ selectedBackend: "llama_cpp", llamaHealthy: true });
  useParamsStore.setState({ globalParams: {}, keepLoaded: false });
  useWorkspacesStore.setState({
    root: "/ws", tree: [], currentPath: "/ws/a.quantamind.yaml",
    current: { name: "a", system: "", user: "Why is the sky blue?", model: null, params: {}, created_at: "t", updated_at: "t", auto_rerun: false },
    dirty: false,
  });
});

describe("SingleRun routes the global params to run_prompt", () => {
  it("sends globalParams (not per-prompt params) on Run", async () => {
    useParamsStore.setState({ globalParams: { temperature: 0, seed: 42 } });
    render(<SingleRun model="llama3.2:1b" />);
    fireEvent.click(screen.getByRole("button", { name: /^run$/i }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("run_prompt", expect.anything()));
    const call = vi.mocked(invoke).mock.calls.find((c) => c[0] === "run_prompt");
    expect(call?.[1]).toMatchObject({ model: "llama3.2:1b", params: { temperature: 0, seed: 42 }, backend: "llama_cpp" });
  });

  it("omits params entirely when globalParams is empty (backend default applies)", async () => {
    render(<SingleRun model="llama3.2:1b" />);
    fireEvent.click(screen.getByRole("button", { name: /^run$/i }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("run_prompt", expect.anything()));
    const call = vi.mocked(invoke).mock.calls.find((c) => c[0] === "run_prompt");
    expect((call?.[1] as Record<string, unknown>).params).toBeUndefined();
  });

  it("omits keepAlive by default and sends -1 when keep-loaded is on", async () => {
    const { unmount } = render(<SingleRun model="llama3.2:1b" />);
    fireEvent.click(screen.getByRole("button", { name: /^run$/i }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("run_prompt", expect.anything()));
    expect((vi.mocked(invoke).mock.calls.find((c) => c[0] === "run_prompt")?.[1] as Record<string, unknown>).keepAlive).toBeUndefined();
    unmount();

    vi.clearAllMocks();
    useParamsStore.setState({ keepLoaded: true });
    render(<SingleRun model="llama3.2:1b" />);
    fireEvent.click(screen.getByRole("button", { name: /^run$/i }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("run_prompt", expect.anything()));
    expect((vi.mocked(invoke).mock.calls.find((c) => c[0] === "run_prompt")?.[1] as Record<string, unknown>).keepAlive).toBe(-1);
  });
});
