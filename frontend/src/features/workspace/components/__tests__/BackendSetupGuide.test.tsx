import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";

vi.mock("@tauri-apps/plugin-shell", () => ({ open: vi.fn() }));
vi.mock("../../hooks/useBackendHealth", () => ({ useMlxBackend: vi.fn() }));

import { useMlxBackend } from "../../hooks/useBackendHealth";
import { BackendSetupGuide } from "../BackendSetupGuide";

const setAppleSilicon = (appleSilicon: boolean) =>
  vi.mocked(useMlxBackend).mockReturnValue({ appleSilicon } as ReturnType<typeof useMlxBackend>);

beforeEach(() => vi.mocked(useMlxBackend).mockReset());

describe("BackendSetupGuide", () => {
  it("shows the MLX (LLM) card on Apple Silicon alongside the cross-platform engines", () => {
    setAppleSilicon(true);
    render(<BackendSetupGuide />);
    expect(screen.getByTestId("setup-engine-mlx")).toBeInTheDocument();
    // llama.cpp (not Apple-only) is always present.
    expect(screen.getByTestId("setup-engine-llama_cpp")).toBeInTheDocument();
  });

  it("shows the venv setup commands for MLX on Apple Silicon", () => {
    setAppleSilicon(true);
    render(<BackendSetupGuide />);
    expect(screen.getByText("python3 -m venv ~/mlx-env")).toBeInTheDocument();
    expect(screen.getByText("source ~/mlx-env/bin/activate")).toBeInTheDocument();
    expect(screen.getByText("pip install -U mlx-lm")).toBeInTheDocument();
  });

  it("excludes the Apple-only MLX LLM card off Apple Silicon, keeps the rest", () => {
    setAppleSilicon(false);
    render(<BackendSetupGuide />);
    expect(screen.queryByTestId("setup-engine-mlx")).toBeNull();
    expect(screen.getByTestId("setup-engine-llama_cpp")).toBeInTheDocument();
    expect(screen.getByTestId("setup-engine-ollama")).toBeInTheDocument();
  });

  it("shows the manual llama.cpp run command WITH --jinja and port 8081", () => {
    setAppleSilicon(false);
    render(<BackendSetupGuide />);
    // A user running their own server must match QuantaMind's flags or generations
    // loop — the command shown must carry --jinja and the :8081 port.
    const cmd = screen.getByText(/^llama-server -m your-model\.gguf/);
    expect(cmd).toHaveTextContent("--jinja");
    expect(cmd).toHaveTextContent("--port 8081");
  });
});
