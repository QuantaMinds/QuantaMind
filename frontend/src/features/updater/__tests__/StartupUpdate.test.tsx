import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";

vi.mock("../../../shared/ipc/system/updater", () => ({
  checkForUpdate: vi.fn(),
  downloadAndInstall: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("../../../shared/ipc/settings/userSettings", () => ({
  getUserSettings: vi.fn(),
  setUserSettings: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("@tauri-apps/plugin-shell", () => ({ open: vi.fn() }));

import { StartupUpdate } from "../components/StartupUpdate";
import { checkForUpdate } from "../../../shared/ipc/system/updater";
import { getUserSettings, setUserSettings } from "../../../shared/ipc/settings/userSettings";

beforeEach(() => vi.clearAllMocks());

const settings = (over = {}) => ({ first_run_complete: false, ...over });

describe("StartupUpdate", () => {
  it("does not check when within the 24h window", async () => {
    vi.mocked(getUserSettings).mockResolvedValue(
      settings({ last_update_check_at: new Date().toISOString() }),
    );
    render(<StartupUpdate />);
    await waitFor(() => expect(getUserSettings).toHaveBeenCalled());
    expect(checkForUpdate).not.toHaveBeenCalled();
    expect(screen.queryByTestId("update-banner")).toBeNull();
  });

  it("checks when due and shows the banner with release notes", async () => {
    vi.mocked(getUserSettings).mockResolvedValue(settings({ last_update_check_at: null }));
    vi.mocked(checkForUpdate).mockResolvedValue({ version: "0.2.0", body: "## New\n- stuff" } as never);
    render(<StartupUpdate />);
    expect(await screen.findByTestId("update-banner")).toBeTruthy();
    expect(screen.getByText(/0\.2\.0 is available/)).toBeTruthy();
    // The 24h stamp is written so we don't recheck on the next launch.
    await waitFor(() => expect(setUserSettings).toHaveBeenCalled());
  });

  it("\"Remind me later\" dismisses without installing", async () => {
    vi.mocked(getUserSettings).mockResolvedValue(settings({ last_update_check_at: null }));
    vi.mocked(checkForUpdate).mockResolvedValue({ version: "0.2.0", body: "" } as never);
    render(<StartupUpdate />);
    fireEvent.click(await screen.findByTestId("update-later"));
    expect(screen.queryByTestId("update-banner")).toBeNull();
  });

  it("shows nothing when already up to date", async () => {
    vi.mocked(getUserSettings).mockResolvedValue(settings({ last_update_check_at: null }));
    vi.mocked(checkForUpdate).mockResolvedValue(null);
    render(<StartupUpdate />);
    await waitFor(() => expect(checkForUpdate).toHaveBeenCalled());
    expect(screen.queryByTestId("update-banner")).toBeNull();
  });
});
