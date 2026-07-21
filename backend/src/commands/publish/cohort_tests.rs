use super::*;
use crate::commands::system::gpu::GpuInfo;

const G: u64 = 1024 * 1024 * 1024;

fn hw(apple: bool, cpu: &str, arch: &str, gpu: GpuInfo, mem_gb: u64) -> HardwareSnapshot {
    HardwareSnapshot {
        total_memory_bytes: mem_gb * G,
        available_memory_bytes: mem_gb * G / 2,
        is_apple_silicon: apple,
        cpu: cpu.to_string(),
        physical_cores: Some(8),
        os_name: None,
        os_version: None,
        arch: arch.to_string(),
        gpu,
        estimated_bandwidth_gbps: None,
    }
}

#[test]
fn apple_silicon_uses_chip_class_and_ram_tier() {
    let h = hw(true, "Apple M3 Pro", "aarch64", GpuInfo::default(), 36);
    assert_eq!(cohort_key(&h), "apple-silicon/m3-pro/32-64gb");
    let h2 = hw(true, "Apple M2", "aarch64", GpuInfo::default(), 16);
    assert_eq!(cohort_key(&h2), "apple-silicon/m2/8-16gb");
}

#[test]
fn discrete_nvidia_gpu_uses_vendor_and_name_slug() {
    let gpu = GpuInfo { name: Some("NVIDIA GeForce RTX 4090".into()), vram_total_bytes: Some(24 * G), vram_free_bytes: None, unified: false, available: true, gpu_working_set_bytes: None };
    let h = hw(false, "AMD Ryzen 9", "x86_64", gpu, 64);
    assert_eq!(cohort_key(&h), "nvidia/nvidia-geforce-rtx-4090/32-64gb");
}

#[test]
fn cpu_only_falls_back_to_arch() {
    let gpu = GpuInfo { name: None, available: false, ..GpuInfo::default() };
    let h = hw(false, "Intel i7", "x86_64", gpu, 16);
    assert_eq!(cohort_key(&h), "cpu/x86-64/8-16gb");
}

#[test]
fn same_hardware_yields_a_stable_key() {
    let mk = || hw(true, "Apple M4 Max", "aarch64", GpuInfo::default(), 128);
    assert_eq!(cohort_key(&mk()), cohort_key(&mk()));
    assert_eq!(cohort_key(&mk()), "apple-silicon/m4-max/64-128gb");
}
