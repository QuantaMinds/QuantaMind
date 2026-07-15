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
import { formatIpcError } from "../../../shared/ipc/core/error";

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

/// Bring-Your-Own task: point the model at ONE of your connected servers with an
/// instruction. There's no answer key, so a BYO run is DIAGNOSTIC only — a live
/// trace + schema-valid rate + fault attribution, never a pass/fail verdict.
export interface McpByoTaskDef {
  name: string;
  instruction: string;
  serverId: string;
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
  /// Controlled-world tasks built in the UI (answer-key scored via Run Batch).
  tasks: McpTaskDef[];
  /// Bring-Your-Own tasks (diagnostic-only; run one at a time against a real
  /// server). Both kinds share the sidebar list.
  byoTasks: McpByoTaskDef[];
  /// A BYO task the user is editing — the builder pre-fills from it (Save replaces the
  /// same-named task). `null` when authoring a fresh one.
  editingByo: McpByoTaskDef | null;
  /// Once a task is saved, the connect/build center collapses to a compact summary
  /// (tasks live in the sidebar; Run Batch runs the world tasks).
  builderCollapsed: boolean;
  setActive: (active: boolean) => void;
  addTask: (task: McpTaskDef) => void;
  removeTask: (name: string) => void;
  addByoTask: (task: McpByoTaskDef) => void;
  removeByoTask: (name: string) => void;
  setEditingByo: (task: McpByoTaskDef | null) => void;
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
  byoTasks: [],
  editingByo: null,
  builderCollapsed: false,

  setActive: (active) => set({ active }),
  addTask: (task) =>
    set((s) => ({ tasks: [...s.tasks.filter((t) => t.name !== task.name), task], builderCollapsed: true })),
  removeTask: (name) => set((s) => ({ tasks: s.tasks.filter((t) => t.name !== name) })),
  addByoTask: (task) =>
    set((s) => ({
      byoTasks: [...s.byoTasks.filter((t) => t.name !== task.name), task],
      builderCollapsed: true,
      editingByo: null,
    })),
  removeByoTask: (name) => set((s) => ({ byoTasks: s.byoTasks.filter((t) => t.name !== name) })),
  setEditingByo: (editingByo) => set({ editingByo, builderCollapsed: editingByo ? false : get().builderCollapsed }),
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
      // formatIpcError, not String(e): a Tauri error is an object, so String(e) renders the
      // useless "[object Object]" — surface the real message the backend sent instead.
      set((s) => ({ probes: { ...s.probes, [id]: { status: "error", error: formatIpcError(e) } } }));
    }
  },
}));
