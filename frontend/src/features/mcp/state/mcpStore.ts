import { create } from "zustand";
import {
  listMcpServers,
  upsertMcpServer,
  removeMcpServer,
  setMcpServerEnabled,
  probeMcpServer,
  type McpServerConfig,
  type McpProbe,
} from "../../../shared/ipc/mcp/servers";

/// The two-track choice, made explicit in the UI. `controlled` = QuantaMind
/// seeds a world we own → full pass/fail task scoring. `byo` = the user's own
/// live servers → only schema + attribution (no answer key), so the UI must NOT
/// offer a task-completion verdict there.
export type TrackMode = "controlled" | "byo";

/// The connect/probe state per server — the "N tools discovered" doctor moment,
/// with a LOUD error surfaced right here (not three minutes into a run).
export interface ProbeState {
  status: "idle" | "probing" | "ok" | "error";
  probe?: McpProbe;
  error?: string;
}

interface McpState {
  /// Whether the MCP source is the active one in the Test page (shared so the
  /// sidebar shows the task list while the center shows the connect/build flow).
  active: boolean;
  servers: McpServerConfig[];
  probes: Record<string, ProbeState>;
  mode: TrackMode;
  loading: boolean;
  setActive: (active: boolean) => void;
  refresh: () => Promise<void>;
  addServer: (cfg: McpServerConfig) => Promise<void>;
  removeServer: (id: string) => Promise<void>;
  setEnabled: (id: string, enabled: boolean) => Promise<void>;
  probe: (id: string) => Promise<void>;
  setMode: (m: TrackMode) => void;
}

export const useMcpStore = create<McpState>((set, get) => ({
  active: false,
  servers: [],
  probes: {},
  mode: "controlled",
  loading: false,

  setActive: (active) => set({ active }),

  refresh: async () => {
    set({ loading: true });
    try {
      set({ servers: await listMcpServers() });
    } finally {
      set({ loading: false });
    }
  },

  addServer: async (cfg) => {
    await upsertMcpServer(cfg);
    await get().refresh();
  },

  removeServer: async (id) => {
    await removeMcpServer(id);
    set((s) => {
      const probes = { ...s.probes };
      delete probes[id];
      return { probes };
    });
    await get().refresh();
  },

  setEnabled: async (id, enabled) => {
    await setMcpServerEnabled(id, enabled);
    await get().refresh();
  },

  probe: async (id) => {
    set((s) => ({ probes: { ...s.probes, [id]: { status: "probing" } } }));
    try {
      const probe = await probeMcpServer(id);
      set((s) => ({ probes: { ...s.probes, [id]: { status: "ok", probe } } }));
    } catch (e) {
      // Fail-fast + loud: a bad command / stdout-polluting server surfaces here.
      set((s) => ({ probes: { ...s.probes, [id]: { status: "error", error: String(e) } } }));
    }
  },

  setMode: (mode) => set({ mode }),
}));
