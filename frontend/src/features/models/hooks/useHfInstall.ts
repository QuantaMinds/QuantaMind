import { useCallback, useEffect, useState } from "react";
import {
  cancelHfInstall,
  installHfGguf,
  type HfPhase,
} from "../../../shared/ipc/models/hf_install";
import { friendlyInstallError } from "../../../shared/install_error";
import { useModelStore } from "../state/modelStore";
import { useInstalledModelsStore } from "../state/installedModelsStore";
import { startDownloadEventBus } from "../state/downloadEventBus";

export type HfStatus = "idle" | "downloading" | "installing" | "success" | "error";

export interface HfInstallState {
  status: HfStatus;
  phase: HfPhase | null;
  percent: number;
  error: string | null;
}

const IDLE: HfInstallState = { status: "idle", phase: null, percent: 0, error: null };

export function useHfInstall() {
  const [local, setLocal] = useState<HfInstallState>(IDLE);
  const activeName = useModelStore((s) => s.activeHfName);
  const entry = useModelStore((s) => (activeName ? s.downloads[activeName] : null));
  const setActiveHfName = useModelStore((s) => s.setActiveHfName);
  const upsertDownload = useModelStore((s) => s.upsertDownload);

  useEffect(() => { void startDownloadEventBus(); }, []);

  const state: HfInstallState = entry
    ? {
        status: entry.status === "downloading" || entry.status === "installing"
          ? entry.status
          : entry.status === "success" ? "success"
          : entry.status === "error" ? "error" : "idle",
        phase: null,
        percent: entry.percent,
        error: entry.error ?? null,
      }
    : local;

  const install = useCallback(async (repo: string, filename: string, name: string) => {
    // Guard: backend only supports one HF install at a time (cancels
    // the prior token). If another install is already in flight,
    // refuse rather than silently clobbering activeHfName — the old
    // download's events would route onto the new entry.
    const store = useModelStore.getState();
    if (store.activeHfName && store.activeHfName !== name) {
      const cur = store.downloads[store.activeHfName];
      if (cur && (cur.status === "downloading" || cur.status === "installing")) {
        const msg = `Another download is in progress (${store.activeHfName}). Cancel it first.`;
        upsertDownload({ id: name, source: "huggingface", name, status: "error", percent: 0, error: msg });
        return;
      }
    }
    setActiveHfName(name);
    upsertDownload({ id: name, source: "huggingface", name, status: "downloading", percent: 0 });
    try {
      // The download lands in the shared weights folder — that IS the install.
      await installHfGguf(repo, filename, name);
      upsertDownload({ id: name, source: "huggingface", name, status: "success", percent: 100 });
      // Don't rely solely on the Tauri `models-changed` broadcast — refresh
      // the installed-models store ourselves so consumers see the new model
      // even if the event is dropped (listener-registration race).
      void useInstalledModelsStore.getState().refresh();
    } catch (e) {
      const msg = friendlyInstallError(e);
      upsertDownload({ id: name, source: "huggingface", name, status: "error", percent: 0, error: msg });
    }
  }, [setActiveHfName, upsertDownload]);

  const reset = useCallback(() => {
    setLocal(IDLE);
    setActiveHfName(null);
  }, [setActiveHfName]);

  const cancel = useCallback(async () => {
    try { await cancelHfInstall(); } catch { /* best-effort */ }
    const n = useModelStore.getState().activeHfName;
    if (n) upsertDownload({ id: n, source: "huggingface", name: n, status: "cancelled", percent: 0 });
  }, [upsertDownload]);

  return { state, install, cancel, reset };
}
