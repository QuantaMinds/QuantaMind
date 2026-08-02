import { listen } from "@tauri-apps/api/event";
import { EVENT_HF_PROGRESS } from "../../../shared/ipc/models/hf_install";
import { EVENT_LOCAL_INSTALL_PROGRESS } from "../../../shared/ipc/models/local_install";
import { EVENT_PULL_PROGRESS } from "../../../shared/ipc/events/pull_events";
import { onHf, onLocal, onPull } from "./downloadEventHandlers";

let starting: Promise<void> | null = null;

export function startDownloadEventBus(): Promise<void> {
  if (starting) return starting;
  starting = (async () => {
    await listen<unknown>(EVENT_HF_PROGRESS, (e) => onHf(e.payload));
    await listen<unknown>(EVENT_PULL_PROGRESS, (e) => onPull(e.payload));
    await listen<unknown>(EVENT_LOCAL_INSTALL_PROGRESS, (e) => onLocal(e.payload));
  })();
  // If the IIFE rejects (transient Tauri init failure), reset the
  // singleton so a subsequent call can retry instead of latching
  // permanently into a rejected promise.
  starting.catch((e) => {
    console.error("downloadEventBus startup failed:", e);
    starting = null;
  });
  return starting;
}

export function __resetDownloadEventBusForTests() {
  starting = null;
}
