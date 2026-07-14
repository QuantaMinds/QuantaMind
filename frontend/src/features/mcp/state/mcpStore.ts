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

/// The one task-file format the guided builder and the JSON-upload path both
/// produce: an instruction + a `world` (seed) + an `oracle` (answer key). Mirrors
/// the backend `McpTask` (inference/eval/mcp) — `mock_result` is gone; we grade
/// the real world.
export interface McpFsSeedFile {
  path: string;
  content: string;
}
export interface McpTaskDef {
  name: string;
  instruction: string;
  world:
    | { type: "fs"; files: McpFsSeedFile[] }
    | { type: "db"; setupSql: string };
  oracle: {
    assert_present?: string[];
    assert_absent?: string[];
    assert_content?: [string, string][];
    assert_eq?: [string, string][];
    assert_contains?: [string, string][];
  };
  k: number;
}

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
  loading: boolean;
  /// Tasks built in the UI (they appear in the sidebar list).
  tasks: McpTaskDef[];
  /// Once a task is saved, the connect/build center collapses to a compact summary
  /// (tasks live in the sidebar; Run Batch runs them).
  builderCollapsed: boolean;
  setActive: (active: boolean) => void;
  addTask: (task: McpTaskDef) => void;
  removeTask: (name: string) => void;
  setBuilderCollapsed: (collapsed: boolean) => void;
  refresh: () => Promise<void>;
  addServer: (cfg: McpServerConfig) => Promise<void>;
  removeServer: (id: string) => Promise<void>;
  setEnabled: (id: string, enabled: boolean) => Promise<void>;
  probe: (id: string) => Promise<void>;
}

export const useMcpStore = create<McpState>((set, get) => ({
  active: false,
  servers: [],
  probes: {},
  loading: false,
  tasks: [],
  builderCollapsed: false,

  setActive: (active) => set({ active }),
  addTask: (task) =>
    set((s) => ({ tasks: [...s.tasks.filter((t) => t.name !== task.name), task], builderCollapsed: true })),
  removeTask: (name) => set((s) => ({ tasks: s.tasks.filter((t) => t.name !== name) })),
  setBuilderCollapsed: (builderCollapsed) => set({ builderCollapsed }),

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
}));
