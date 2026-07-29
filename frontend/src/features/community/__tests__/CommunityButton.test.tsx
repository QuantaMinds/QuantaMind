import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/plugin-shell", () => ({ open: vi.fn().mockResolvedValue(undefined) }));
vi.mock("../../../shared/ipc/settings/userSettings", () => ({
  getUserSettings: vi.fn().mockResolvedValue({ community_prompt_shown: false }),
  setUserSettings: vi.fn().mockResolvedValue(undefined),
}));

import { CommunityButton } from "../components/CommunityButton";
import { useCommunityStore } from "../state/communityStore";
import { DISCORD_INVITE_URL, GITHUB_REPO_URL, X_PROFILE_URL } from "../links";
import { open } from "@tauri-apps/plugin-shell";
import { getUserSettings, setUserSettings } from "../../../shared/ipc/settings/userSettings";

let onLine = true;

beforeEach(() => {
  vi.clearAllMocks();
  useCommunityStore.setState({ promptShown: null });
  vi.mocked(getUserSettings).mockResolvedValue({ community_prompt_shown: false } as never);
  onLine = true;
  vi.spyOn(window.navigator, "onLine", "get").mockImplementation(() => onLine);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("CommunityButton", () => {
  it("auto-opens the invite once for a fresh install, and dismiss persists the flag", async () => {
    render(<CommunityButton />);
    await waitFor(() => expect(screen.getByTestId("community-popover")).toBeTruthy());
    fireEvent.click(screen.getByTestId("community-dismiss"));
    expect(screen.queryByTestId("community-popover")).toBeNull();
    await waitFor(() =>
      expect(setUserSettings).toHaveBeenCalledWith(
        expect.objectContaining({ community_prompt_shown: true }),
      ),
    );
  });

  it("does not auto-open for a returning user", async () => {
    vi.mocked(getUserSettings).mockResolvedValue({ community_prompt_shown: true } as never);
    render(<CommunityButton />);
    await waitFor(() => expect(useCommunityStore.getState().promptShown).toBe(true));
    expect(screen.queryByTestId("community-popover")).toBeNull();
  });

  it("does not auto-open while offline, but the button still works", async () => {
    onLine = false;
    render(<CommunityButton />);
    await waitFor(() => expect(useCommunityStore.getState().promptShown).toBe(false));
    expect(screen.queryByTestId("community-popover")).toBeNull();
    fireEvent.click(screen.getByTestId("community-button"));
    expect(screen.getByTestId("community-popover")).toBeTruthy();
  });

  it("Join the Discord opens the invite in the OS browser and closes the popover", async () => {
    render(<CommunityButton />);
    await waitFor(() => expect(screen.getByTestId("community-popover")).toBeTruthy());
    fireEvent.click(screen.getByTestId("community-discord"));
    expect(open).toHaveBeenCalledWith(DISCORD_INVITE_URL);
    expect(screen.queryByTestId("community-popover")).toBeNull();
  });

  it("Follow on X opens the profile in the OS browser", async () => {
    render(<CommunityButton />);
    await waitFor(() => expect(screen.getByTestId("community-popover")).toBeTruthy());
    fireEvent.click(screen.getByTestId("community-x"));
    expect(open).toHaveBeenCalledWith(X_PROFILE_URL);
  });

  it("Star on GitHub opens the repo in the OS browser and closes the popover", async () => {
    render(<CommunityButton />);
    await waitFor(() => expect(screen.getByTestId("community-popover")).toBeTruthy());
    fireEvent.click(screen.getByTestId("community-github"));
    expect(open).toHaveBeenCalledWith(GITHUB_REPO_URL);
    expect(screen.queryByTestId("community-popover")).toBeNull();
  });

  it("Escape dismisses the popover", async () => {
    render(<CommunityButton />);
    await waitFor(() => expect(screen.getByTestId("community-popover")).toBeTruthy());
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByTestId("community-popover")).toBeNull();
  });
});
