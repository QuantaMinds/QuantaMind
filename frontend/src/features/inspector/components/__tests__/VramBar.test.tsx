import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { VramBar } from "../VramBar";

const GB = 1024 ** 3;

describe("VramBar", () => {
  it("shows 'not available' with no entry", () => {
    render(<VramBar />);
    expect(screen.getByTestId("vram-na")).toBeInTheDocument();
    expect(screen.queryByTestId("vram-bar")).toBeNull();
  });

  it("scales the footprint against unified memory and labels it", () => {
    render(
      <VramBar
        entry={{ name: "m", size_bytes: 4 * GB, size_vram_bytes: 4 * GB, context_length: 4096 }}
        deviceTotalBytes={16 * GB}
        unified
      />,
    );
    expect(screen.getByTestId("vram-seg-used")).toHaveStyle({ width: "25%" });
    expect(screen.getByText(/in unified memory of 16.0GB \(25%\)/)).toBeInTheDocument();
    expect(screen.getByText(/4096 ctx/)).toBeInTheDocument();
  });

  it("notes offload to RAM for a partially-offloaded discrete GPU", () => {
    render(<VramBar entry={{ name: "m", size_bytes: 1000, size_vram_bytes: 600 }} deviceTotalBytes={4000} />);
    expect(screen.getByText(/in VRAM of/)).toBeInTheDocument();
    expect(screen.getByText(/offloaded to RAM/)).toBeInTheDocument();
  });

  it("renders the real /api/ps footprint honestly: pressure marker is a static amber planning estimate, never an OOM claim", () => {
    // REAL bytes captured live from Ollama /api/ps for llama-3.2-1b-instruct:iq3_m
    // (0.66 GB weights + preallocated 4096-ctx KV cache = the loaded footprint).
    // Nothing measures where this machine OOMs, so the UI must not claim an "OOM
    // ceiling" — the marker is the backend's PRESSURE_FRACTION planning threshold.
    const { container } = render(
      <VramBar
        entry={{ name: "llama-3.2-1b-instruct:iq3_m", size_bytes: 1077278720, size_vram_bytes: 1077278720, context_length: 4096 }}
        deviceTotalBytes={16 * GB}
        unified
      />,
    );
    // The displayed number is exactly what /api/ps reported (1.0GB), not an invention.
    expect(screen.getByText(/1\.0GB in unified memory of 16\.0GB \(6%\)/)).toBeInTheDocument();
    // No OOM claim anywhere; the marker is labelled as the planning threshold.
    expect(container.textContent).not.toMatch(/OOM/i);
    expect(screen.getAllByTitle(/planning estimate/).length).toBeGreaterThan(0);
    expect(screen.getByText(/pressure threshold \(planning estimate\)/)).toBeInTheDocument();
    // Static, not alarming: no pulse animation, no red on the marker.
    expect(container.innerHTML).not.toContain("animate-pulse");
    expect(container.innerHTML).not.toContain("bg-red-600");
  });
});
