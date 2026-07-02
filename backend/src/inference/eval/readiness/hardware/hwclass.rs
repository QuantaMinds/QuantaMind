use crate::inference::eval::agentic::spec::Tier;
use serde::{Deserialize, Serialize};

/// Hardware class from total system memory — the bar that calibrates the required
/// difficulty tier. A 16 GB laptop is judged on Easy/Medium; a 128 GB Studio on
/// Hard/Extreme. Same engine, the bar rises with the machine. Serializes as a
/// stable snake_case string (`mainstream`, …) — it rides on the publish payload as
/// the hardware-class advisory, so the canonical hash needs a fixed wire form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardwareClass {
    Constrained,
    Mainstream,
    Workstation,
    Frontier,
}

const GIB: u64 = 1024 * 1024 * 1024;

/// Round total bytes to the nearest whole GiB. This is the buffer zone: minor OS
/// memory-reporting variance can't flip-flop a machine across a class boundary —
/// 15.5 GiB rounds to 16 (Mainstream), not down to 15 (Constrained).
pub fn gb_round(total_bytes: u64) -> u32 {
    ((total_bytes + GIB / 2) / GIB) as u32
}

/// Map memory (in rounded GB) to a hardware class. Boundaries: <16 / 16–32 / 33–96 / 96+.
pub fn classify(total_mem_gb: u32) -> HardwareClass {
    match total_mem_gb {
        0..=15 => HardwareClass::Constrained,
        16..=32 => HardwareClass::Mainstream,
        33..=96 => HardwareClass::Workstation,
        _ => HardwareClass::Frontier,
    }
}

/// Classify straight from the hardware snapshot's byte count (rounds first).
pub fn classify_bytes(total_bytes: u64) -> HardwareClass {
    classify(gb_round(total_bytes))
}

/// The agentic-eval context window (`num_ctx`) ceiling for a hardware class — the ONE place
/// hardware adaptivity belongs. Reasoning models need a large window to HOLD their (fixed,
/// machine-independent) per-turn budget + the growing transcript; a bigger machine can afford a
/// bigger window. This does NOT change the budget or the tier difficulty — it only decides
/// whether the fixed budget FITS. A box whose ceiling can't hold the budget yields an honest
/// `Truncated` (a true result on that hardware). Each class's ceiling comfortably holds the tier
/// it's expected to run (`default_required_tier`): Mainstream→Medium, Workstation→Hard, etc.
pub fn agentic_ctx_ceiling(total_bytes: u64) -> u32 {
    match classify_bytes(total_bytes) {
        HardwareClass::Constrained => 8192,
        HardwareClass::Mainstream => 16384,
        HardwareClass::Workstation => 32768,
        // Frontier is 65536 (not 49152) so the Extreme Deep preset (32768 thinking) genuinely fits:
        // a late Extreme turn holds ~10–15k accumulated transcript + 32768 thinking + ~2k answer
        // ≈ 45–50k, which 49152 would truncate even on the flagship. Extreme Deep therefore needs a
        // Frontier host; on a Workstation it honestly reads `Truncated (context-bound)`.
        HardwareClass::Frontier => 65536,
    }
}

/// The difficulty tier a class is expected to clear by default. Seeds the built-in
/// readiness profiles; users can still edit/save their own `required_tier`.
pub fn default_required_tier(c: HardwareClass) -> Tier {
    match c {
        HardwareClass::Constrained => Tier::Medium,
        HardwareClass::Mainstream => Tier::Medium,
        HardwareClass::Workstation => Tier::Hard,
        HardwareClass::Frontier => Tier::Extreme,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_boundaries_are_exact() {
        assert_eq!(classify(15), HardwareClass::Constrained);
        assert_eq!(classify(16), HardwareClass::Mainstream);
        assert_eq!(classify(32), HardwareClass::Mainstream);
        assert_eq!(classify(33), HardwareClass::Workstation);
        assert_eq!(classify(96), HardwareClass::Workstation);
        assert_eq!(classify(97), HardwareClass::Frontier);
    }

    #[test]
    fn rounding_buffers_a_near_boundary_machine_up() {
        // 15.5 GiB of reported memory must not be read as a 15 GB Constrained box.
        let bytes = (15 * GIB) + (GIB / 2);
        assert_eq!(gb_round(bytes), 16);
        assert_eq!(classify_bytes(bytes), HardwareClass::Mainstream);
        // A clean 128 GiB Studio is Frontier.
        assert_eq!(classify_bytes(128 * GIB), HardwareClass::Frontier);
    }

    #[test]
    fn required_tier_rises_with_the_class() {
        assert_eq!(default_required_tier(HardwareClass::Constrained), Tier::Medium);
        assert_eq!(default_required_tier(HardwareClass::Mainstream), Tier::Medium);
        assert_eq!(default_required_tier(HardwareClass::Workstation), Tier::Hard);
        assert_eq!(default_required_tier(HardwareClass::Frontier), Tier::Extreme);
    }
}
