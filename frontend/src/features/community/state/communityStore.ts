import { create } from "zustand";
import { getUserSettings, setUserSettings } from "../../../shared/ipc/settings/userSettings";

interface CommunityState {
  promptShown: boolean | null; // null until loaded
  load: () => Promise<void>;
  markShown: () => Promise<void>;
}

/// Tracks the one-time community invite (user_settings.community_prompt_shown).
/// Fails "shown" (promptShown=true) if settings can't load, so a backend hiccup
/// never re-spams the invite.
export const useCommunityStore = create<CommunityState>((set) => ({
  promptShown: null,
  load: async () => {
    try {
      const s = await getUserSettings();
      set({ promptShown: s.community_prompt_shown });
    } catch {
      set({ promptShown: true });
    }
  },
  markShown: async () => {
    set({ promptShown: true });
    try {
      const s = await getUserSettings();
      await setUserSettings({ ...s, community_prompt_shown: true });
    } catch (e) {
      console.error("community prompt persist failed:", e);
    }
  },
}));
