import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";

vi.mock("@tauri-apps/plugin-shell", () => ({ open: vi.fn() }));

import { BackendSetupGuide } from "../BackendSetupGuide";

describe("BackendSetupGuide", () => {
  it("shows a card for every supported engine", () => {
    render(<BackendSetupGuide />);
    expect(screen.getByTestId("setup-engine-llama_cpp")).toBeInTheDocument();
    expect(screen.getByTestId("setup-engine-vllm")).toBeInTheDocument();
    expect(screen.getByTestId("setup-engine-vllm")).toBeInTheDocument();
  });

  it("shows the manual llama.cpp run command WITH --jinja and port 8081", () => {
    render(<BackendSetupGuide />);
    // A user running their own server must match QuantaMind's flags or generations
    // loop — the command shown must carry --jinja and the :8081 port.
    const cmd = screen.getByText(/^llama-server -m your-model\.gguf/);
    expect(cmd).toHaveTextContent("--jinja");
    expect(cmd).toHaveTextContent("--port 8081");
  });
});
