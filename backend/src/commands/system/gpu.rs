use crate::os::{EngineHost, Host};
use serde::Serialize;
use std::sync::OnceLock;

const MIB: u64 = 1024 * 1024;

/// GPU/VRAM info, best-effort per platform. `unified` = shared memory (Apple
/// Silicon: no separate VRAM pool, so `vram_*` stay None and the UI reports
/// system RAM). `available:false` = couldn't probe → "Not available", never
/// fabricated. See `docs/architecture.md#robustness`.
#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct GpuInfo {
    pub name: Option<String>,
    pub vram_total_bytes: Option<u64>,
    pub vram_free_bytes: Option<u64>,
    pub unified: bool,
    pub available: bool,
}

/// Parse one `nvidia-smi --query-gpu=name,memory.total,memory.free
/// --format=csv,noheader,nounits` line ("RTX 4090, 24576, 3210") into
/// (name, total_mib, free_mib). Pure.
pub fn parse_nvidia_csv(line: &str) -> Option<(String, u64, u64)> {
    let parts: Vec<&str> = line.split(',').map(str::trim).collect();
    if parts.len() < 3 || parts[0].is_empty() {
        return None;
    }
    Some((parts[0].to_string(), parts[1].parse().ok()?, parts[2].parse().ok()?))
}

fn nvidia() -> Option<GpuInfo> {
    let mut cmd = Host::command("nvidia-smi");
    cmd.args(["--query-gpu=name,memory.total,memory.free", "--format=csv,noheader,nounits"]);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let (name, total_mib, free_mib) = parse_nvidia_csv(text.lines().next()?)?;
    Some(GpuInfo {
        name: Some(name),
        vram_total_bytes: Some(total_mib * MIB),
        vram_free_bytes: Some(free_mib * MIB),
        unified: false,
        available: true,
    })
}

/// Parse `rocm-smi --showmeminfo vram --showproductname --json` output —
/// takes the first `card*` entry, extracts VRAM total bytes and card name.
/// Values may be present as either JSON numbers or strings (rocm-smi has
/// varied across versions), so accept both. Pure.
pub fn parse_rocm_smi_json(bytes: &[u8]) -> Option<(String, u64, Option<u64>)> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let obj = v.as_object()?;
    let card = obj
        .iter()
        .find(|(k, _)| k.starts_with("card"))
        .map(|(_, val)| val)?
        .as_object()?;
    let read_bytes = |k: &str| -> Option<u64> {
        let val = card.get(k)?;
        val.as_u64().or_else(|| val.as_str().and_then(|s| s.parse().ok()))
    };
    let total = read_bytes("VRAM Total Memory (B)")?;
    let used = read_bytes("VRAM Total Used Memory (B)");
    let free = used.map(|u| total.saturating_sub(u));
    let name = card
        .get("Card Series")
        .or_else(|| card.get("Card Model"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| "AMD GPU".to_string());
    Some((name, total, free))
}

fn amd() -> Option<GpuInfo> {
    let mut cmd = Host::command("rocm-smi");
    cmd.args(["--showmeminfo", "vram", "--showproductname", "--json"]);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let (name, total, free) = parse_rocm_smi_json(&out.stdout)?;
    Some(GpuInfo {
        name: Some(name),
        vram_total_bytes: Some(total),
        vram_free_bytes: free,
        unified: false,
        available: true,
    })
}

/// Parse `xpu-smi discovery -j` — takes the first entry in `device_list`.
/// `memory_physical_size_byte` is total; xpu-smi doesn't report free, so it's
/// left `None` (never fabricated). Pure.
pub fn parse_xpu_smi_json(bytes: &[u8]) -> Option<(String, u64)> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let dev = v.get("device_list")?.as_array()?.first()?;
    let name = dev.get("device_name")?.as_str()?.to_string();
    let total = dev
        .get("memory_physical_size_byte")
        .and_then(|x| x.as_u64().or_else(|| x.as_str().and_then(|s| s.parse().ok())))?;
    Some((name, total))
}

fn intel_xpu() -> Option<GpuInfo> {
    let mut cmd = Host::command("xpu-smi");
    cmd.args(["discovery", "-j"]);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let (name, total) = parse_xpu_smi_json(&out.stdout)?;
    Some(GpuInfo {
        name: Some(name),
        vram_total_bytes: Some(total),
        // xpu-smi's discovery view doesn't include free memory — leave None
        // rather than fabricate.
        vram_free_bytes: None,
        unified: false,
        available: true,
    })
}

/// Windows last-resort GPU probe via DXGI adapter enumeration. Returns the
/// primary adapter's name + dedicated VRAM. **`vram_free_bytes` stays `None`**
/// — DXGI exposes no free-VRAM API, and the "never fabricate" rule forbids
/// guessing. Fires when vendor CLIs (nvidia-smi, rocm-smi, xpu-smi) aren't
/// installed but a physical GPU is present.
#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
fn dxgi() -> Option<GpuInfo> {
    use ::windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};

    // SAFETY: `CreateDXGIFactory1` and adapter-desc calls are standard COM
    // patterns; each interface pointer is only used until its own `Drop`
    // releases it (windows crate handles the release automatically). We never
    // dereference a raw pointer that we could hand out beyond this fn.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let adapter: IDXGIAdapter1 = factory.EnumAdapters1(0).ok()?;
        let desc = adapter.GetDesc1().ok()?;
        // `Description` is a fixed-size UTF-16 buffer; find its NUL terminator.
        let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
        let name = String::from_utf16_lossy(&desc.Description[..len]);
        // Skip the software "Microsoft Basic Render Driver" adapter — it's not
        // a real GPU and its DedicatedVideoMemory is 0.
        if desc.DedicatedVideoMemory == 0 || name.contains("Basic Render") {
            return None;
        }
        Some(GpuInfo {
            name: Some(name),
            vram_total_bytes: Some(desc.DedicatedVideoMemory as u64),
            vram_free_bytes: None,
            unified: false,
            available: true,
        })
    }
}

#[cfg(not(target_os = "windows"))]
fn dxgi() -> Option<GpuInfo> {
    None
}

#[cfg(target_os = "macos")]
fn apple() -> Option<GpuInfo> {
    let mut cmd = Host::command("sysctl");
    cmd.args(["-n", "machdep.cpu.brand_string"]);
    let out = cmd.output().ok()?;
    let chip = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || chip.is_empty() {
        return None;
    }
    // Apple Silicon GPU is the same SoC; memory is unified (no separate pool).
    Some(GpuInfo { name: Some(format!("{chip} (integrated)")), unified: true, available: true, ..Default::default() })
}

#[cfg(not(target_os = "macos"))]
fn apple() -> Option<GpuInfo> {
    None
}

/// Memoized GPU probe. Each vendor branch is a subprocess spawn, and
/// `get_hardware_snapshot` is called by ~7 components that all mount at launch
/// (App.tsx keeps every tab mounted), so an uncached probe fired the vendor CLIs
/// in a burst — the launch-time stall. GPU identity and total VRAM are static for
/// a session, so the whole `GpuInfo` is cached after the first successful probe.
/// Tradeoff: `vram_free_bytes` freezes at first-probe time; a short-TTL refresh is
/// a future consideration (see `docs/process.md#future-considerations`).
pub fn probe_gpu() -> GpuInfo {
    static CACHE: OnceLock<GpuInfo> = OnceLock::new();
    CACHE.get_or_init(probe_gpu_uncached).clone()
}

/// Try NVIDIA (cross-OS via nvidia-smi), then AMD (rocm-smi), then Intel
/// (xpu-smi), then Windows DXGI fallback, then Apple Silicon. Anything past
/// the last successful probe is skipped. Otherwise an unavailable GpuInfo.
fn probe_gpu_uncached() -> GpuInfo {
    nvidia().or_else(amd).or_else(intel_xpu).or_else(dxgi).or_else(apple).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nvidia_csv_into_name_and_mib() {
        let r = parse_nvidia_csv("NVIDIA GeForce RTX 4090, 24576, 3210").unwrap();
        assert_eq!(r, ("NVIDIA GeForce RTX 4090".into(), 24576, 3210));
    }

    #[test]
    fn rejects_malformed_csv() {
        assert!(parse_nvidia_csv("").is_none());
        assert!(parse_nvidia_csv("name, notanumber, 5").is_none());
        assert!(parse_nvidia_csv(", 1, 2").is_none());
    }

    #[test]
    fn parse_rocm_smi_json_accepts_string_and_numeric_bytes() {
        // rocm-smi 5.x reports VRAM as strings; 6.x sometimes numeric.
        let json_str = br#"{"card0":{"VRAM Total Memory (B)":"17163091968","VRAM Total Used Memory (B)":"1073741824","Card Series":"Radeon RX 7900 XTX"}}"#;
        let (name, total, free) = parse_rocm_smi_json(json_str).unwrap();
        assert_eq!(name, "Radeon RX 7900 XTX");
        assert_eq!(total, 17_163_091_968);
        assert_eq!(free, Some(17_163_091_968 - 1_073_741_824));

        let json_num = br#"{"card0":{"VRAM Total Memory (B)":8589934592,"Card Series":"Radeon RX 6800"}}"#;
        let (name, total, free) = parse_rocm_smi_json(json_num).unwrap();
        assert_eq!(name, "Radeon RX 6800");
        assert_eq!(total, 8_589_934_592);
        assert_eq!(free, None); // no used → no free (never fabricated)
    }

    #[test]
    fn parse_rocm_smi_json_rejects_malformed() {
        assert!(parse_rocm_smi_json(b"").is_none());
        assert!(parse_rocm_smi_json(b"not json").is_none());
        assert!(parse_rocm_smi_json(b"{}").is_none()); // no card* key
        assert!(parse_rocm_smi_json(b"{\"card0\":{}}").is_none()); // no total
    }

    #[test]
    fn parse_xpu_smi_json_extracts_first_device() {
        let json = br#"{
            "device_list": [
                {"device_id": 0, "device_name": "Intel(R) Arc(TM) A770 Graphics", "memory_physical_size_byte": 17179869184}
            ]
        }"#;
        let (name, total) = parse_xpu_smi_json(json).unwrap();
        assert_eq!(name, "Intel(R) Arc(TM) A770 Graphics");
        assert_eq!(total, 17_179_869_184);
    }

    #[test]
    fn parse_xpu_smi_json_rejects_malformed() {
        assert!(parse_xpu_smi_json(b"").is_none());
        assert!(parse_xpu_smi_json(b"{}").is_none());
        assert!(parse_xpu_smi_json(b"{\"device_list\":[]}").is_none());
        assert!(parse_xpu_smi_json(b"{\"device_list\":[{}]}").is_none());
    }

    #[test]
    fn probe_never_panics() {
        let _ = probe_gpu();
    }

    #[test]
    fn probe_gpu_is_memoized_and_consistent() {
        // The burst of launch-time callers must all see the same cached value.
        let first = probe_gpu();
        let second = probe_gpu();
        assert_eq!(first, second);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn dxgi_never_panics() {
        let _ = dxgi();
    }
}
