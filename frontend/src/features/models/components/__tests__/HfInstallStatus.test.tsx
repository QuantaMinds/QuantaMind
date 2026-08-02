import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { HfInstallStatus } from "../HfInstallStatus";
import type { HfInstallState } from "../../hooks/useHfInstall";

const base = (
  overrides: Partial<HfInstallState> = {},
): HfInstallState => ({
  status: "idle",
  phase: null,
  percent: 0,
  error: null,
  ...overrides,
});

describe("HfInstallStatus", () => {
  it("renders nothing in idle state", () => {
    const { container } = render(
      <HfInstallStatus state={base()} onCancel={() => {}} onReset={() => {}} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders progress bar + percent + cancel button while downloading", () => {
    const onCancel = vi.fn();
    render(
      <HfInstallStatus
        state={base({ status: "downloading", percent: 42 })}
        onCancel={onCancel}
        onReset={() => {}}
      />,
    );
    const el = screen.getByTestId("hf-downloading");
    expect(el.querySelector("progress")).toHaveAttribute("value", "42");
    expect(el).toHaveTextContent("42%");
    fireEvent.click(screen.getByRole("button", { name: /cancel/i }));
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it("renders explicit success banner with a dismiss button on success", () => {
    const onReset = vi.fn();
    render(
      <HfInstallStatus
        state={base({ status: "success", percent: 100 })}
        onCancel={() => {}}
        onReset={onReset}
      />,
    );
    expect(screen.getByTestId("hf-success")).toHaveTextContent(/Installed ✓/);
    fireEvent.click(screen.getByRole("button", { name: /dismiss/i }));
    expect(onReset).toHaveBeenCalledOnce();
  });

  it("renders error message with dismiss on error", () => {
    const onReset = vi.fn();
    render(
      <HfInstallStatus
        state={base({ status: "error", error: "registration rolled back" })}
        onCancel={() => {}}
        onReset={onReset}
      />,
    );
    expect(screen.getByTestId("hf-error")).toHaveTextContent("registration rolled back");
    fireEvent.click(screen.getByRole("button", { name: /dismiss/i }));
    expect(onReset).toHaveBeenCalledOnce();
  });
});
