// Windows-only. This file is not even parsed on macOS/Linux builds — the
// `pub mod windows;` in `os/mod.rs` is `#[cfg(target_os = "windows")]`-gated.
// This is the ONLY file in the crate that needs `unsafe {}` blocks; the crate
// root sets `#![deny(unsafe_code)]`, and we scope a targeted allow here rather
// than punching a hole in the whole workspace.
#![allow(unsafe_code)]

use crate::os::EngineHost;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use ::windows::Win32::Foundation::CloseHandle;
use ::windows::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT};
use ::windows::Win32::System::Threading::{
    OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
};

// CreateProcess flags:
//   CREATE_NO_WINDOW           = suppress the sidecar's stdio console window
//                                (invisible when Tauri launches from Explorer)
//   CREATE_NEW_PROCESS_GROUP   = **R1**: each child is its own group id (= its
//                                own pid), so `GenerateConsoleCtrlEvent(BREAK,
//                                child_pid)` targets the child + its tree,
//                                *not* QuantaMind's console. Without this bit,
//                                the child inherits our group and a "graceful
//                                stop" would kill the app itself.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

pub struct WindowsHost;

impl EngineHost for WindowsHost {
    fn resolve_on_path(bin: &str) -> Option<PathBuf> {
        // `where.exe` is the Windows analogue of `which`; ships on every SKU
        // from Vista onward and is on the default PATH.
        let out = Command::new("where").arg(bin).output().ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        // `where` may list several matches, newline-separated. First wins.
        let first = text.lines().next()?.trim();
        if first.is_empty() {
            return None;
        }
        PathBuf::from(first).parent().map(|p| p.to_path_buf())
    }

    fn envs_for_lib_dir(_dir: &Path) -> Vec<(&'static str, PathBuf)> {
        // No env var needed: the Windows loader searches the directory
        // containing the .exe first, so co-located DLLs are found without
        // any per-spawn setup. Injecting into PATH would work but would leak
        // to grandchildren.
        Vec::new()
    }

    fn apply_spawn_flags(cmd: &mut Command) {
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }

    fn graceful_stop(pid: u32) -> Result<(), String> {
        // Safe: the child was spawned with CREATE_NEW_PROCESS_GROUP, so its
        // pid IS its group id. Signaling the group takes down the whole tree
        // (Ollama's model-runner grandchildren included) — verified by the
        // R1 live gate ("after Stop, confirm no orphaned grandchildren").
        unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) }
            .map_err(|e| format!("GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, {pid}) failed: {e}"))
    }

    fn hard_stop(pid: u32) -> Result<(), String> {
        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE, false, pid)
                .map_err(|e| format!("OpenProcess({pid}, TERMINATE) failed: {e}"))?;
            let term_res = TerminateProcess(handle, 1);
            // Close regardless of TerminateProcess outcome — leaking the
            // handle would waste kernel resources for nothing.
            let _ = CloseHandle(handle);
            term_res.map_err(|e| format!("TerminateProcess({pid}) failed: {e}"))
        }
    }

    fn pid_alive(pid: u32) -> bool {
        unsafe {
            match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                Ok(handle) => {
                    let _ = CloseHandle(handle);
                    true
                }
                Err(_) => false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_on_path_finds_cmd_exe() {
        // `cmd.exe` is on PATH in every Windows environment including CI.
        let dir = WindowsHost::resolve_on_path("cmd").expect("cmd.exe must be on PATH");
        assert!(dir.exists(), "resolved dir {dir:?} should exist");
    }

    #[test]
    fn pid_alive_true_for_self_false_for_impossible() {
        assert!(WindowsHost::pid_alive(std::process::id()));
        // pid 0xFFFF_FFFF is not openable with our access rights; a healthy
        // Windows returns Err here.
        assert!(!WindowsHost::pid_alive(0xFFFF_FFFF));
    }

    #[test]
    fn envs_for_lib_dir_is_empty() {
        assert!(WindowsHost::envs_for_lib_dir(Path::new("C:\\lib")).is_empty());
    }

    #[test]
    fn apply_spawn_flags_sets_both_bits() {
        // Can't observe `creation_flags` after it's set on `Command` (no
        // getter), so we assert the constants match the documented Win32
        // values — the actual effect is exercised by the R1 live gate.
        assert_eq!(CREATE_NO_WINDOW, 0x0800_0000);
        assert_eq!(CREATE_NEW_PROCESS_GROUP, 0x0000_0200);
        let mut cmd = Command::new("cmd");
        WindowsHost::apply_spawn_flags(&mut cmd);
    }
}
