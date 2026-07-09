import { useCallback, useEffect, useState } from "react";
import type { Update } from "@tauri-apps/plugin-updater";
import {
  checkForUpdate,
  downloadAndInstall,
  getCurrentVersion,
} from "../../../shared/ipc/system/updater";
import { formatIpcError } from "../../../shared/ipc/core/error";

export type UpdaterStatus =
  | "idle"
  | "checking"
  | "up_to_date"
  | "available"
  | "downloading"
  | "installing"
  | "error";

export function useUpdater() {
  const [status, setStatus] = useState<UpdaterStatus>("idle");
  const [currentVersion, setCurrentVersion] = useState<string | null>(null);
  const [update, setUpdate] = useState<Update | null>(null);
  const [downloaded, setDownloaded] = useState(0);
  const [total, setTotal] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void getCurrentVersion().then(setCurrentVersion).catch(() => {});
  }, []);

  const check = useCallback(async () => {
    setError(null);
    setStatus("checking");
    try {
      const found = await checkForUpdate();
      if (found) {
        setUpdate(found);
        setStatus("available");
      } else {
        setUpdate(null);
        setStatus("up_to_date");
      }
    } catch (e) {
      setError(formatIpcError(e));
      setStatus("error");
    }
  }, []);

  const install = useCallback(async () => {
    if (!update) return;
    setError(null);
    setStatus("downloading");
    setDownloaded(0);
    setTotal(null);
    try {
      await downloadAndInstall(update, (d, t) => {
        setDownloaded(d);
        setTotal(t);
        if (t && d >= t) setStatus("installing");
      });
    } catch (e) {
      setError(formatIpcError(e));
      setStatus("error");
    }
  }, [update]);

  return { status, currentVersion, update, downloaded, total, error, check, install };
}
