import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

/// Stable OS labels used by the setup-card copy switch and error-info
/// classifier. `"unknown"` is the safe fallback for anything the backend
/// returns outside the supported three — the frontend treats it as Linux
/// for copy purposes (the least Homebrew/winget-specific guidance).
export const HostOsSchema = z.enum(["mac", "windows", "linux", "unknown"]);
export type HostOs = z.infer<typeof HostOsSchema>;

const RustOsSchema = z.string();

/// One-shot Tauri invoke that reads the running OS. Backend returns
/// `std::env::consts::OS` (`"macos" | "windows" | "linux"` for our supported
/// set). Normalized to the stable frontend labels here so components don't
/// need to know Rust conventions.
export async function osPlatform(): Promise<HostOs> {
  try {
    const raw = RustOsSchema.parse(await invoke("os_platform"));
    switch (raw) {
      case "macos":
        return "mac";
      case "windows":
        return "windows";
      case "linux":
        return "linux";
      default:
        return "unknown";
    }
  } catch {
    // If the IPC fails (dev without Tauri, an old backend without the
    // command wired in), fall back to `"unknown"` — copy defaults treat it
    // as the least-installer-specific case.
    return "unknown";
  }
}
