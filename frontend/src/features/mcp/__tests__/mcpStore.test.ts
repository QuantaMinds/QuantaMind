import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../../shared/ipc/mcp/servers", () => ({
  listMcpServers: vi.fn(),
  upsertMcpServer: vi.fn().mockResolvedValue(undefined),
  removeMcpServer: vi.fn().mockResolvedValue(undefined),
  setMcpServerEnabled: vi.fn().mockResolvedValue(undefined),
  probeMcpServer: vi.fn(),
}));

import { useMcpStore } from "../state/mcpStore";
import {
  listMcpServers,
  probeMcpServer,
  upsertMcpServer,
  type McpServerConfig,
} from "../../../shared/ipc/mcp/servers";

const cfg: McpServerConfig = {
  id: "filesystem",
  command: "npx",
  args: ["-y", "@modelcontextprotocol/server-filesystem"],
  env_keys: [],
  roots: [],
  enabled: true,
};

beforeEach(() => {
  useMcpStore.setState({ servers: [], probes: {}, loading: false, tasks: [], byoTasks: [], activeByo: null, builderCollapsed: false });
  vi.clearAllMocks();
});

describe("mcpStore", () => {
  it("refresh loads the server list", async () => {
    vi.mocked(listMcpServers).mockResolvedValue([cfg]);
    await useMcpStore.getState().refresh();
    expect(useMcpStore.getState().servers).toEqual([cfg]);
  });

  it("probe success records the 'N tools discovered' result", async () => {
    vi.mocked(probeMcpServer).mockResolvedValue({
      server_name: "secure-filesystem-server",
      protocol_version: "2025-06-18",
      tool_count: 14,
      tool_names: ["read_text_file"],
    });
    await useMcpStore.getState().probe("filesystem");
    const p = useMcpStore.getState().probes["filesystem"];
    expect(p.status).toBe("ok");
    expect(p.probe?.tool_count).toBe(14);
  });

  it("probe failure surfaces a LOUD error, never a silent hang", async () => {
    vi.mocked(probeMcpServer).mockRejectedValue(
      new Error("server closed the connection (logging to stdout?)"),
    );
    await useMcpStore.getState().probe("bad");
    const p = useMcpStore.getState().probes["bad"];
    expect(p.status).toBe("error");
    expect(p.error).toContain("stdout");
  });

  it("addServer upserts then refreshes", async () => {
    vi.mocked(listMcpServers).mockResolvedValue([cfg]);
    await useMcpStore.getState().addServer(cfg);
    expect(upsertMcpServer).toHaveBeenCalledWith(cfg);
    expect(useMcpStore.getState().servers).toEqual([cfg]);
  });

  it("addTask saves a world task and collapses the builder", () => {
    const task = { name: "t1", instruction: "do it", world: { type: "fs" as const, files: [] }, oracle: {}, k: 10 };
    useMcpStore.getState().addTask(task);
    expect(useMcpStore.getState().tasks).toEqual([task]);
    expect(useMcpStore.getState().builderCollapsed).toBe(true);
  });

  it("addByoTask saves a diagnostic task alongside world tasks, and collapses", () => {
    useMcpStore.getState().addByoTask({ name: "b1", instruction: "list files", serverId: "filesystem" });
    expect(useMcpStore.getState().byoTasks).toEqual([{ name: "b1", instruction: "list files", serverId: "filesystem" }]);
    expect(useMcpStore.getState().builderCollapsed).toBe(true);
  });

  it("removeByoTask drops the task and clears activeByo when it was open", () => {
    useMcpStore.getState().addByoTask({ name: "b1", instruction: "x", serverId: "fs" });
    useMcpStore.getState().setActiveByo("b1");
    useMcpStore.getState().removeByoTask("b1");
    expect(useMcpStore.getState().byoTasks).toEqual([]);
    expect(useMcpStore.getState().activeByo).toBeNull();
  });
});
