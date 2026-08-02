import { create } from "zustand";
import { getUserSettings } from "../../../shared/ipc/settings/userSettings";

/// The configured remote OpenAI endpoint (vLLM), mirrored from UserSettings so the health
/// pollers can gate on them REACTIVELY: an unconfigured remote backend is never probed (the old
/// code hit a blank endpoint every 5s → constant connection-refused). Loaded once at startup
/// (BackendSelector) and updated when the Settings form saves a new endpoint.
type RemoteEndpointsStore = {
  vllmUrl: string | null;
  load: () => Promise<void>;
  setUrls: (urls: { vllmUrl: string | null | undefined }) => void;
};

const clean = (u: string | null | undefined): string | null => {
  const t = (u ?? "").trim();
  return t.length > 0 ? t : null;
};

export const useRemoteEndpointsStore = create<RemoteEndpointsStore>((set) => ({
  vllmUrl: null,
  load: async () => {
    try {
      const s = await getUserSettings();
      set({ vllmUrl: clean(s.vllm_url) });
    } catch {
      set({ vllmUrl: null });
    }
  },
  setUrls: ({ vllmUrl }) => set({ vllmUrl: clean(vllmUrl) }),
}));
