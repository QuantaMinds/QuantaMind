import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn().mockResolvedValue("0.1.0") }));

import { check } from "@tauri-apps/plugin-updater";
import { UpdateChecker } from "../components/UpdateChecker";

beforeEach(() => {
  vi.mocked(check).mockReset();
});

const fakeUpdate = (over: Partial<{ version: string; body: string; date: string }> = {}) => {
  const downloadAndInstall = vi.fn().mockResolvedValue(undefined);
  return {
    version: over.version ?? "0.1.1",
    body: over.body ?? "Bug fixes.",
    date: over.date,
    downloadAndInstall,
  };
};

describe("UpdateChecker", () => {
  it("shows the current version pulled from getVersion()", async () => {
    render(<UpdateChecker />);
    await waitFor(() =>
      expect(screen.getByTestId("update-current-version")).toHaveTextContent(/v0\.1\.0/),
    );
  });

  it("'up_to_date' state when check() returns null", async () => {
    vi.mocked(check).mockResolvedValue(null);
    render(<UpdateChecker />);
    fireEvent.click(screen.getByTestId("update-check-button"));
    expect(await screen.findByTestId("update-up-to-date")).toBeInTheDocument();
  });

  it("'available' state surfaces version + body + Install button", async () => {
    vi.mocked(check).mockResolvedValue(fakeUpdate({ version: "0.2.0", body: "New temperature popover." }) as never);
    render(<UpdateChecker />);
    fireEvent.click(screen.getByTestId("update-check-button"));
    const card = await screen.findByTestId("update-available");
    expect(card).toHaveTextContent(/v0\.2\.0/);
    expect(card).toHaveTextContent(/New temperature popover/);
    expect(screen.getByTestId("update-install-button")).toBeInTheDocument();
  });

  it("'error' state surfaces a check() rejection", async () => {
    vi.mocked(check).mockRejectedValue({ kind: "inference", message: "signature mismatch" });
    render(<UpdateChecker />);
    fireEvent.click(screen.getByTestId("update-check-button"));
    expect(await screen.findByTestId("update-error")).toHaveTextContent(/signature mismatch/);
  });

  it("disables the Check button while in flight", async () => {
    let resolve!: (v: unknown) => void;
    vi.mocked(check).mockImplementation(
      () => new Promise<unknown>((r) => { resolve = r; }) as never,
    );
    render(<UpdateChecker />);
    fireEvent.click(screen.getByTestId("update-check-button"));
    await waitFor(() => expect(screen.getByTestId("update-check-button")).toBeDisabled());
    resolve(null);
  });
});
