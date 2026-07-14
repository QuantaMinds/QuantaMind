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
  useMcpStore.setState({ servers: [], probes: {}, mode: "controlled", loading: false });
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

  it("setMode switches between the controlled and bring-your-own tracks", () => {
    expect(useMcpStore.getState().mode).toBe("controlled");
    useMcpStore.getState().setMode("byo");
    expect(useMcpStore.getState().mode).toBe("byo");
  });
});
