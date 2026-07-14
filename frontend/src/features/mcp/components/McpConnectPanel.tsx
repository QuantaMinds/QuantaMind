import { useEffect, useState } from "react";
import { useMcpStore } from "../state/mcpStore";
import type { McpServerConfig } from "../../../shared/ipc/mcp/servers";

/// Connect servers (with the loud "N tools discovered" preflight) before defining
/// a task to score in a controlled world.
export function McpConnectPanel() {
  const { servers, probes, refresh, addServer, removeServer, probe } = useMcpStore();

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <div className="flex flex-col gap-4">
      <ServerList servers={servers} probes={probes} onProbe={probe} onRemove={removeServer} />
      <AddServerForm onAdd={addServer} />
    </div>
  );
}

function ServerList({
  servers,
  probes,
  onProbe,
  onRemove,
}: {
  servers: McpServerConfig[];
  probes: ReturnType<typeof useMcpStore.getState>["probes"];
  onProbe: (id: string) => void;
  onRemove: (id: string) => void;
}) {
  if (servers.length === 0) {
    return <p className="text-sm opacity-60">No MCP servers yet — add one below or upload an mcp.json.</p>;
  }
  return (
    <ul className="flex flex-col gap-2">
      {servers.map((s) => {
        const p = probes[s.id];
        return (
          <li key={s.id} className="flex items-center gap-3 rounded-lg border border-neutral-700 p-2">
            <span className="font-mono text-sm">{s.id}</span>
            <span className="text-xs opacity-60">
              {s.command} {s.args.join(" ")}
            </span>
            <span className="ml-auto text-xs">
              {p?.status === "probing" && "connecting…"}
              {p?.status === "ok" && (
                <span className="text-emerald-400">✓ {p.probe?.tool_count} tools discovered</span>
              )}
              {p?.status === "error" && <span className="text-red-400" title={p.error}>✗ {p.error}</span>}
            </span>
            <button type="button" className="rounded border border-neutral-600 px-2 py-1 text-xs" onClick={() => onProbe(s.id)}>
              Connect
            </button>
            <button type="button" className="rounded border border-neutral-600 px-2 py-1 text-xs" onClick={() => onRemove(s.id)}>
              Remove
            </button>
          </li>
        );
      })}
    </ul>
  );
}

const TEMPLATES: Record<string, { id: string; command: string; args: string }> = {
  filesystem: { id: "filesystem", command: "npx", args: "-y @modelcontextprotocol/server-filesystem" },
  sqlite: { id: "sqlite", command: "npx", args: "-y mcp-server-sqlite-npx" },
};

function AddServerForm({ onAdd }: { onAdd: (cfg: McpServerConfig) => void }) {
  const [id, setId] = useState("");
  const [command, setCommand] = useState("npx");
  const [args, setArgs] = useState("");
  const fill = (t: keyof typeof TEMPLATES) => {
    setId(TEMPLATES[t].id);
    setCommand(TEMPLATES[t].command);
    setArgs(TEMPLATES[t].args);
  };

  const submit = () => {
    if (!id.trim() || !command.trim()) return;
    onAdd({
      id: id.trim(),
      command: command.trim(),
      args: args.split(/\s+/).filter(Boolean),
      env_keys: [],
      roots: [],
      enabled: true,
    });
    setId("");
    setArgs("");
  };

  return (
    <div className="flex flex-wrap items-center gap-2">
      <span className="text-xs opacity-60">quick add:</span>
      <button type="button" className="rounded border border-neutral-600 px-2 py-0.5 text-xs" onClick={() => fill("filesystem")}>filesystem</button>
      <button type="button" className="rounded border border-neutral-600 px-2 py-0.5 text-xs" onClick={() => fill("sqlite")}>sqlite</button>
      <div className="w-full" />
      <input className="rounded border border-neutral-600 bg-transparent px-2 py-1 text-sm" placeholder="id" value={id} onChange={(e) => setId(e.target.value)} />
      <input className="rounded border border-neutral-600 bg-transparent px-2 py-1 text-sm" placeholder="command" value={command} onChange={(e) => setCommand(e.target.value)} />
      <input className="flex-1 rounded border border-neutral-600 bg-transparent px-2 py-1 text-sm" placeholder="args (space-separated)" value={args} onChange={(e) => setArgs(e.target.value)} />
      <button type="button" className="rounded bg-neutral-200 px-3 py-1 text-sm text-neutral-900" onClick={submit}>
        Add
      </button>
    </div>
  );
}
