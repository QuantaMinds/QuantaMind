import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";

vi.mock("../../hooks/useStartLlamaServer", () => ({ useStartLlamaServer: () => ({ start: vi.fn(), status: "idle", error: null }) }));
vi.mock("../../hooks/useStopLlamaServer", () => ({ useStopLlamaServer: () => ({ stop: vi.fn(), status: "idle" }) }));

import { ServerControl } from "../status/ServerControl";
import { useBackendStore } from "../../../../shared/state/backendStore";

beforeEach(() => useBackendStore.setState({ selectedBackend: "llama_cpp", llamaHealthy: null }));

describe("ServerControl (header)", () => {
  it("shows the llama.cpp Start/Stop when llama.cpp is active", () => {
    render(<ServerControl />);
    expect(screen.getByTestId("llama-start")).toBeInTheDocument();
  });

  it("shows a read-only readout for a remote backend — the app can't start it", () => {
    useBackendStore.setState({ selectedBackend: "vllm" });
    render(<ServerControl />);
    expect(screen.queryByTestId("llama-start")).toBeNull();
  });
});
