import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

const RssSchema = z.number().int().nonnegative().nullable();

/// Total resident memory (bytes) of the local inference server process, or null
/// if it isn't running / isn't measurable. Best-effort — never throws.
export async function localServerRss(): Promise<number | null> {
  try {
    return RssSchema.parse(await invoke("get_local_server_rss"));
  } catch (e) {
    console.error("get_local_server_rss failed:", e);
    return null;
  }
}
