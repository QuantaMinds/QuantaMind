import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../../shared/ipc/settings/userSettings", () => ({
  getUserSettings: vi.fn(),
  setUserSettings: vi.fn().mockResolvedValue(undefined),
}));

import { useOnboardingStore } from "../state/onboardingStore";
import { getUserSettings, setUserSettings } from "../../../shared/ipc/settings/userSettings";

beforeEach(() => {
  vi.clearAllMocks();
  useOnboardingStore.setState({ complete: null });
});

describe("onboardingStore", () => {
  it("load reflects a fresh install (not complete)", async () => {
    vi.mocked(getUserSettings).mockResolvedValue({ first_run_complete: false, community_prompt_shown: false });
    await useOnboardingStore.getState().load();
    expect(useOnboardingStore.getState().complete).toBe(false);
  });

  it("load reflects a returning user (complete)", async () => {
    vi.mocked(getUserSettings).mockResolvedValue({ first_run_complete: true, community_prompt_shown: false });
    await useOnboardingStore.getState().load();
    expect(useOnboardingStore.getState().complete).toBe(true);
  });

  it("fails open if settings can't load (never traps the user)", async () => {
    vi.mocked(getUserSettings).mockRejectedValue(new Error("boom"));
    await useOnboardingStore.getState().load();
    expect(useOnboardingStore.getState().complete).toBe(true);
  });

  it("finish flips the flag and persists it", async () => {
    vi.mocked(getUserSettings).mockResolvedValue({ first_run_complete: false, community_prompt_shown: false });
    await useOnboardingStore.getState().finish();
    expect(useOnboardingStore.getState().complete).toBe(true);
    expect(setUserSettings).toHaveBeenCalledWith(expect.objectContaining({ first_run_complete: true }));
  });
});
