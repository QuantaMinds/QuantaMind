#![deny(clippy::unwrap_used)]
use crate::commands::system::gpu::{probe_gpu, GpuInfo};
use crate::commands::system::hardware_mem::{compute_available, guess_memory_bandwidth_gbps};
use crate::errors::AppError;
use crate::sync::MutexExt;
use serde::Serialize;
use std::sync::{Mutex, OnceLock};
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct HardwareSnapshot {
    pub total_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub is_apple_silicon: bool,
    pub cpu: String,
    pub physical_cores: Option<usize>,
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub arch: String,
    pub gpu: GpuInfo,
    /// Nominal unified-memory bandwidth (GB/s) for known Apple-Silicon chips;
    /// `None` when unrecognised (shown as "Not available", never fabricated).
    pub estimated_bandwidth_gbps: Option<u32>,
}

fn system() -> &'static Mutex<System> {
    static SYSTEM: OnceLock<Mutex<System>> = OnceLock::new();
    SYSTEM.get_or_init(|| {
        Mutex::new(System::new_with_specifics(
            RefreshKind::new()
                .with_memory(MemoryRefreshKind::everything())
                .with_cpu(CpuRefreshKind::everything()),
        ))
    })
}

pub fn snapshot() -> HardwareSnapshot {
    let mut sys = system().lock_recover();
    sys.refresh_memory();
    let total = sys.total_memory();
    let available = compute_available(total, sys.available_memory(), sys.used_memory());
    let cpu = sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default();
    let estimated_bandwidth_gbps = guess_memory_bandwidth_gbps(&cpu);
    HardwareSnapshot {
        total_memory_bytes: total,
        available_memory_bytes: available,
        is_apple_silicon: cfg!(all(target_os = "macos", target_arch = "aarch64")),
        cpu,
        physical_cores: sys.physical_core_count(),
        os_name: System::name(),
        os_version: System::os_version(),
        arch: std::env::consts::ARCH.to_string(),
        gpu: probe_gpu(),
        estimated_bandwidth_gbps,
    }
}

/// Async so the launch-time burst of callers doesn't tie up Tauri's synchronous
/// command worker pool: `snapshot()` blocks on `sysinfo` + a memoized GPU probe,
/// so it runs on the blocking pool and the first call warms the cache for the rest.
#[tauri::command]
pub async fn get_hardware_snapshot() -> Result<HardwareSnapshot, AppError> {
    tokio::task::spawn_blocking(snapshot).await.map_err(|e| AppError::Internal(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_reports_nonzero_total_and_an_arch() {
        let s = snapshot();
        assert!(s.total_memory_bytes > 0);
        assert!(!s.arch.is_empty());
    }

    #[test]
    fn available_at_most_total() {
        let s = snapshot();
        assert!(s.available_memory_bytes <= s.total_memory_bytes);
    }

    #[test]
    fn apple_silicon_flag_matches_cfg() {
        assert_eq!(snapshot().is_apple_silicon, cfg!(all(target_os = "macos", target_arch = "aarch64")));
    }
}
