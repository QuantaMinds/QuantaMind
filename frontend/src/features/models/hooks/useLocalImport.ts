import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { listLlamaModels } from "../../../shared/ipc/models/llama_start";
import {
  inspectGguf,
  installLocalGguf,
  type GgufMetadata,
} from "../../../shared/ipc/models/gguf";
import { useModelStore } from "../state/modelStore";
import { useInstalledModelsStore } from "../state/installedModelsStore";
import { formatIpcError } from "../../../shared/ipc/core/error";
import { startDownloadEventBus } from "../state/downloadEventBus";

const defaultName = (path: string) =>
  (path.split("/").pop() ?? path).replace(/\.gguf$/i, "").replace(/[^A-Za-z0-9_\-.:]/g, "-");

export function useLocalImport() {
  const pendingPath = useModelStore((s) => s.pendingLocalPath);
  const setPendingPath = useModelStore((s) => s.setPendingLocalPath);
  const activeLocalName = useModelStore((s) => s.activeLocalName);
  const setActiveLocalName = useModelStore((s) => s.setActiveLocalName);
  const entry = useModelStore((s) => (activeLocalName ? s.downloads[activeLocalName] : null));
  const [path, setPath] = useState<string | null>(null);
  const [meta, setMeta] = useState<GgufMetadata | null>(null);
  const [name, setName] = useState("");
  const [installed, setInstalled] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);

  useEffect(() => { void startDownloadEventBus(); }, []);

  // Names already installed, so the form can warn before overwriting one.
  useEffect(() => {
    listLlamaModels()
      .then((m) => setInstalled(new Set(m.map((x) => x.name))))
      .catch((e) => console.error("useLocalImport: listLlamaModels failed —", formatIpcError(e)));
  }, []);

  const choose = useCallback(async (p: string) => {
    setError(null); setPath(p); setName(defaultName(p));
    try { setMeta(await inspectGguf(p)); }
    catch (e) { setError(formatIpcError(e)); setMeta(null); }
  }, []);

  useEffect(() => {
    if (pendingPath && pendingPath !== path) {
      void choose(pendingPath);
      setPendingPath(null);
    }
  }, [pendingPath, path, choose, setPendingPath]);

  const browse = useCallback(async () => {
    const picked = await open({ multiple: false, filters: [{ name: "GGUF", extensions: ["gguf"] }] });
    if (typeof picked === "string") await choose(picked);
  }, [choose]);

  const cancel = useCallback(() => {
    setPath(null); setMeta(null); setName(""); setError(null);
    setActiveLocalName(null);
  }, [setActiveLocalName]);

  const upsert = useModelStore((st) => st.upsertDownload);
  const doImport = useCallback(async () => {
    if (!path) return;
    setError(null);
    setActiveLocalName(name);
    try {
      await installLocalGguf(path, name);
      upsert({ id: name, source: "local", name, status: "success", percent: 100 });
      // Self-heal — don't trust the broadcast event alone.
      void useInstalledModelsStore.getState().refresh();
      cancel();
    } catch (e) {
      const msg = formatIpcError(e);
      setError(msg);
      upsert({ id: name, source: "local", name, status: "error", percent: 0, error: msg });
      setActiveLocalName(null);
    }
  }, [path, name, cancel, setActiveLocalName, upsert]);

  const busy = !!entry && entry.status === "installing";
  const percent = entry?.percent ?? 0;
  const phaseLabel = entry?.phaseLabel ?? null;

  return {
    path, meta, name, error, busy, percent, phaseLabel,
    conflict: installed.has(name),
    setName, browse, cancel, doImport,
  };
}
