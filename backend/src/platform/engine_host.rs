use std::path::{Path, PathBuf};
use std::process::Command;

/// Platform-adapter contract used by every runtime engine (Ollama, llama.cpp,
/// whisper, future engines). One impl per OS family; the concrete choice is a
/// compile-time `type Host = …` in `host.rs`. Every method is an *associated
/// function* — no `&self`, no state, no runtime dispatch.
///
/// Every method has an OS-specific failure mode called out in the plan's
/// Live-only risks section; see each method's doc for which one.
pub trait EngineHost {
    /// Locate `bin` on the user's PATH. Returns the *directory* containing the
    /// binary, matching the shape existing callers use (they join `bin_name()`
    /// onto the result). A GUI-launched Tauri app doesn't inherit the shell
    /// PATH, so callers layer their own well-known prefixes on top.
    fn resolve_on_path(bin: &str) -> Option<PathBuf>;

    /// Environment variables a spawned sidecar needs so its co-located dylibs
    /// resolve. macOS = DYLD_FALLBACK_LIBRARY_PATH; Linux = LD_LIBRARY_PATH;
    /// Windows = empty (the loader finds DLLs sitting next to the .exe).
    fn envs_for_lib_dir(dir: &Path) -> Vec<(&'static str, PathBuf)>;

    /// Set OS-specific spawn flags on `cmd` before `.spawn()`. On Windows this
    /// MUST include CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP — R1: the
    /// group flag is what makes `graceful_stop` target the child instead of
    /// killing QuantaMind itself. No-op on Unix.
    fn apply_spawn_flags(cmd: &mut Command);

    /// Best-effort request that pid exit cleanly. Unix: SIGTERM. Windows:
    /// GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) — safe only because
    /// `apply_spawn_flags` put each child in its own group.
    fn graceful_stop(pid: u32) -> Result<(), String>;

    /// Force kill. Unix: SIGKILL. Windows: TerminateProcess.
    fn hard_stop(pid: u32) -> Result<(), String>;

    /// True while pid exists and is signalable.
    fn pid_alive(pid: u32) -> bool;
}
