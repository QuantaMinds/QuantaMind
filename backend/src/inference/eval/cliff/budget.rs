use crate::inference::eval::agentic::difficulty::passk::{think_tokens_for_preset, ThinkPreset};
use crate::inference::eval::agentic::spec::Tier;

/// Output-token floor per probe turn — only a tool call is expected, never prose.
/// Every model gets this; a thinking model gets a depth-scaled scratchpad on top.
pub const CLIFF_ANSWER_TOKENS: u32 = 256;

/// Context the probe reserves ON TOP of the padding for the system prompt (tool
/// schemas), the injected needle, and the non-thinking answer floor. A thinking
/// run additionally reserves its scratchpad — `CliffBudget::headroom` is the one
/// place that sum lives, for the GUI command, the CLI, and the frontend mirror.
pub const CLIFF_BASE_HEADROOM: u32 = 2048;

/// Map a rung's padded depth to the difficulty-tier band whose thinking budget it
/// borrows. The cliff has no tiers of its own — a deeper context is "harder" the
/// same way a higher tier is (more to hold while reasoning), so the scratchpad
/// scales with depth through the SAME canonical `think_tokens_for_preset` table
/// the Tests page uses, never a second set of numbers. Bands: ≤4k → Easy,
/// ≤8k → Medium, ≤16k → Hard, deeper → Extreme. The unpadded baseline (0) is Easy.
pub fn tier_for_depth(target_tokens: u32) -> Tier {
    match target_tokens {
        0..=4096 => Tier::Easy,
        4097..=8192 => Tier::Medium,
        8193..=16384 => Tier::Hard,
        _ => Tier::Extreme,
    }
}

/// The probe's per-turn output budget: answer floor for every model, plus — for a
/// thinking model — the depth-banded reasoning scratchpad. Defaults to the
/// non-thinking budget, which is byte-identical to the pre-feature behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CliffBudget {
    pub is_thinking: bool,
    pub preset: ThinkPreset,
}

impl CliffBudget {
    /// `num_predict` for one probe turn at this rung's target depth.
    pub fn max_output_for(self, target_tokens: u32) -> u32 {
        CLIFF_ANSWER_TOKENS + self.scratchpad(target_tokens)
    }

    /// Context headroom the run must reserve above `max_tokens` (the deepest rung):
    /// base (system + needle + answer floor) plus the deepest rung's scratchpad.
    pub fn headroom(self, max_tokens: u32) -> u32 {
        CLIFF_BASE_HEADROOM + self.scratchpad(max_tokens)
    }

    /// The reasoning-scratchpad portion at a depth — 0 for a non-thinking run.
    fn scratchpad(self, target_tokens: u32) -> u32 {
        if self.is_thinking {
            think_tokens_for_preset(tier_for_depth(target_tokens), self.preset)
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_bands_map_to_tiers_at_exact_boundaries() {
        assert_eq!(tier_for_depth(0), Tier::Easy);
        assert_eq!(tier_for_depth(4096), Tier::Easy);
        assert_eq!(tier_for_depth(4097), Tier::Medium);
        assert_eq!(tier_for_depth(8192), Tier::Medium);
        assert_eq!(tier_for_depth(8193), Tier::Hard);
        assert_eq!(tier_for_depth(16384), Tier::Hard);
        assert_eq!(tier_for_depth(16385), Tier::Extreme);
    }

    #[test]
    fn non_thinking_budget_is_the_flat_answer_floor_at_every_depth() {
        let b = CliffBudget::default();
        for depth in [0, 4096, 9216, 65536] {
            assert_eq!(b.max_output_for(depth), CLIFF_ANSWER_TOKENS);
        }
        assert_eq!(b.headroom(65536), CLIFF_BASE_HEADROOM);
    }

    #[test]
    fn thinking_budget_scales_monotonically_with_depth_and_preset() {
        for preset in [ThinkPreset::Lean, ThinkPreset::Standard, ThinkPreset::Deep] {
            let b = CliffBudget { is_thinking: true, preset };
            let by_depth: Vec<u32> = [0, 6144, 12288, 32768].iter().map(|&d| b.max_output_for(d)).collect();
            for w in by_depth.windows(2) {
                assert!(w[0] < w[1], "budget must grow with depth: {by_depth:?}");
            }
        }
        let at = |p, d| CliffBudget { is_thinking: true, preset: p }.max_output_for(d);
        for d in [0, 6144, 12288, 32768] {
            assert!(at(ThinkPreset::Lean, d) < at(ThinkPreset::Standard, d));
            assert!(at(ThinkPreset::Standard, d) < at(ThinkPreset::Deep, d));
        }
    }

    #[test]
    fn thinking_headroom_reserves_the_deepest_rungs_scratchpad() {
        let b = CliffBudget { is_thinking: true, preset: ThinkPreset::Standard };
        // 9216 falls in the Hard band → Standard scratchpad 10240 on top of the base.
        assert_eq!(b.headroom(9216), CLIFF_BASE_HEADROOM + 10240);
        // Matches the budget identity: headroom = base + (max_output − answer floor).
        assert_eq!(b.headroom(9216), CLIFF_BASE_HEADROOM + b.max_output_for(9216) - CLIFF_ANSWER_TOKENS);
    }
}
