import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../../shared/ipc/settings/userSettings", () => ({
  getUserSettings: vi.fn(),
  setUserSettings: vi.fn().mockResolvedValue(undefined),
}));

import { useCommunityStore } from "../state/communityStore";
import { getUserSettings, setUserSettings } from "../../../shared/ipc/settings/userSettings";

beforeEach(() => {
  vi.clearAllMocks();
  useCommunityStore.setState({ promptShown: null });
});

describe("communityStore", () => {
  it("load reflects a fresh install (not shown)", async () => {
    vi.mocked(getUserSettings).mockResolvedValue({ community_prompt_shown: false } as never);
    await useCommunityStore.getState().load();
    expect(useCommunityStore.getState().promptShown).toBe(false);
  });

  it("load reflects a returning user (shown)", async () => {
    vi.mocked(getUserSettings).mockResolvedValue({ community_prompt_shown: true } as never);
    await useCommunityStore.getState().load();
    expect(useCommunityStore.getState().promptShown).toBe(true);
  });

  it("fails shown if settings can't load (never re-spams the invite)", async () => {
    vi.mocked(getUserSettings).mockRejectedValue(new Error("boom"));
    await useCommunityStore.getState().load();
    expect(useCommunityStore.getState().promptShown).toBe(true);
  });

  it("markShown flips the flag and persists it", async () => {
    vi.mocked(getUserSettings).mockResolvedValue({ community_prompt_shown: false } as never);
    await useCommunityStore.getState().markShown();
    expect(useCommunityStore.getState().promptShown).toBe(true);
    expect(setUserSettings).toHaveBeenCalledWith(
      expect.objectContaining({ community_prompt_shown: true }),
    );
  });
});
