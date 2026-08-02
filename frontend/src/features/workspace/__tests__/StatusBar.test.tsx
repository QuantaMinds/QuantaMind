import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";

import { StatusBar } from "../components/status/StatusBar";
import { useWorkspaceStore } from "../state/workspaceStore";
import { useBackendStore } from "../../../shared/state/backendStore";

describe("StatusBar", () => {
  beforeEach(() => {
    useWorkspaceStore.setState({ lastRunMetrics: null });
    useBackendStore.setState({ selectedBackend: "llama_cpp", llamaHealthy: null });
  });

  it("renders 'no run yet' before any run completes", async () => {
    render(<StatusBar model={null} />);
    expect(screen.getByTestId("status-bar-metrics")).toHaveTextContent(
      "no run yet",
    );
  });

  it("names the running model on the llama.cpp backend", async () => {
    useBackendStore.setState({ selectedBackend: "llama_cpp", llamaHealthy: true });
    render(<StatusBar model="phi3" />);
    const status = await screen.findByLabelText("llama.cpp health");
    expect(status).toHaveTextContent("llama.cpp · running (phi3)");
    expect(status.querySelector("span")?.className).toMatch(/bg-green-500/);
  });

  it("on the llama.cpp backend, shows 'not started' when the server is down", async () => {
    useBackendStore.setState({ selectedBackend: "llama_cpp", llamaHealthy: false });
    render(<StatusBar model="phi3" />);
    const status = await screen.findByLabelText("llama.cpp health");
    expect(status).toHaveTextContent("llama.cpp · not started");
    expect(status.querySelector("span")?.className).toMatch(/bg-red-500/);
  });

  it("renders metrics from the store with correct precision", () => {
    useWorkspaceStore.setState({
      lastRunMetrics: { ttft_ms: 137, tokens_per_sec: 47.345, token_count: 92, timeline: [] },
    });
    render(<StatusBar model="llama3.2:1b" />);
    const m = screen.getByTestId("status-bar-metrics");
    expect(m).toHaveTextContent("TTFT 137ms");
    expect(m).toHaveTextContent("47.3 tok/s");
    expect(m).toHaveTextContent("92 tokens");
  });

  it("invokes onModelClick when the model name is clicked", () => {
    const onClick = vi.fn();
    render(<StatusBar model="mistral:7b" onModelClick={onClick} />);
    fireEvent.click(screen.getByRole("button", { name: "mistral:7b" }));
    expect(onClick).toHaveBeenCalledTimes(1);
  });

});
