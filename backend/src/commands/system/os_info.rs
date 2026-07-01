// Tauri command that returns the running OS as a stable string. The frontend
// reads this once at boot via `useHostOs` and branches setup-card copy per OS.
// Kept in `commands/system/` (not `os/`) because `os/` is the platform-adapter
// module for lifecycle code; this is a Tauri IPC command.

/// The running OS, as a stable lowercase string: `"macos" | "windows" |
/// "linux"` (or `std::env::consts::OS` for anything else — the frontend hook
/// coerces unknowns to `"linux"` as the safest fallback for setup copy).
#[tauri::command]
pub fn os_platform() -> &'static str {
    std::env::consts::OS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_a_known_os_string() {
        let os = os_platform();
        // std::env::consts::OS is always one of: "linux", "macos", "windows",
        // "android", "ios", "freebsd", "openbsd", "netbsd", "dragonfly",
        // "solaris", "haiku", "vxworks", "emscripten", "unknown". Our supported
        // set is the first three; any other value would surprise the frontend
        // classifier — assert we're in the supported family.
        assert!(
            matches!(os, "macos" | "windows" | "linux"),
            "os_platform() returned unexpected value {os:?} — frontend hook expects mac/windows/linux"
        );
    }
}
