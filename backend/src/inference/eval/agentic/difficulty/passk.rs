use crate::inference::eval::agentic::spec::Tier;

/// Pass^k scales with difficulty: a harder task demands more independent successes
/// to be credited. This is the cheapest re-separation lever for big models — τ-bench
/// shows top models cluster at pass^1 but spread at pass^8. `Easy = 5` is exactly
/// the pre-Phase-9 default, so an untiered task's k is unchanged.
///
/// Precedence at the call site (`build.rs`): an explicit `spec.k` (authored or the
/// UI K override) wins; only an absent `k` falls back to this tier policy.
pub fn pass_k_for(tier: Tier) -> u32 {
    match tier {
        Tier::Easy => 5,
        Tier::Medium => 8,
        Tier::Hard => 16,
        Tier::Extreme => 24,
    }
}

/// The agentic step budget scales with difficulty: a harder task has a longer horizon
/// (more checkpoints, prereqs, decoys), so it needs room to work before the loop cap
/// fires. `Easy = 8` matches the pre-Phase-9 UI default, so an Easy/untiered run is
/// unchanged. Chosen so each tier's window stays within the memory-safe `num_ctx`
/// ceiling (`agentic_num_ctx`): 8/16/32/48 → 5120/8192/14336/16384 tokens, the last at
/// the clamp — past ~38 steps `num_ctx` is pinned at the ceiling anyway, so `Extreme`'s
/// extra budget buys deeper trajectories without inflating the KV cache further.
///
/// Same precedence as `k` (`build.rs`): an explicit `spec.max_steps` (authored or the UI
/// Max-Steps field) wins; only an absent value falls back to this tier policy.
pub fn max_steps_for(tier: Tier) -> u32 {
    match tier {
        Tier::Easy => 8,
        Tier::Medium => 16,
        Tier::Hard => 32,
        Tier::Extreme => 48,
    }
}

/// The per-turn output-token budget (`num_predict`) is SEPARATELY budgeted: a *solution/answer*
/// portion every model gets, plus a *reasoning scratchpad* portion added only for thinking models.
/// The old flat 256 cap for non-thinking models was a structural failure guarantee — a
/// `write_file(content=…)` payload can't fit in 256 tokens, so the call truncated mid-string,
/// parsed to zero calls, and scored Malformed/Hallucinated regardless of model SIZE. Likewise a
/// reasoning model's `<think>` is charged against the SAME `num_predict` as the answer (Ollama
/// caps the TOTAL — proven live: qwen3.5:9b spent ~3700/4096 tokens thinking and returned an empty
/// answer), so an undersized scratchpad starves the answer. Token budget is a hidden correctness
/// parameter; sizing it so truncation approaches zero lets the eval measure capability, not the cap.
/// Pure I/O plumbing, NOT a difficulty axis: `pass_k_for` / `max_steps_for` / decoys / traps are
/// unchanged, so the tiers stay exactly as hard.

/// The solution/answer budget: enough for a TYPICAL BATCHED turn (several tool calls in one JSON
/// array), NOT a single file. The FLOOR for EVERY model, thinking or not — the deliverable is
/// identical. Independent of the thinking preset (only the scratchpad portion varies by preset).
pub fn answer_tokens_for(tier: Tier) -> u32 {
    match tier {
        Tier::Easy => 1536,
        Tier::Medium => 2048,
        Tier::Hard => 2560,
        Tier::Extreme => 3072,
    }
}

/// The user-selectable thinking-budget preset (frontend sidebar). NOT a free-form token slider —
/// a continuous knob makes verdicts un-comparable across users; three fixed presets keep every
/// verdict reproducible and labeled ("Ready @ Standard"). `Deep` is a large FIXED cap, never
/// "unlimited". Default `Standard`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThinkPreset {
    Lean,
    #[default]
    Standard,
    Deep,
}

/// The reasoning-scratchpad budget for a (tier, preset), ADDED only for thinking models. A FIXED,
/// generous, finite per-turn allowance — deliberately NOT hardware-scaled: reasoning length is a
/// property of the TASK and MODEL, not the tester's RAM, so scaling by hardware would let the same
/// model pass a tier on a big box and fail on a small one → a non-reproducible tier (the one thing
/// the leaderboard must never do). Hardware adaptivity lives ONLY in `num_ctx` (whether the window
/// can HOLD this budget); a box that genuinely can't is an honest `Truncated (context-bound)`.
///
/// These are PLACEHOLDERS until §5 locks them from a cross-MODEL histogram (P95 of the chattiest
/// model per tier — models vary ~2.5×). Hard/Extreme are biased up but still likely low (the
/// 15–18k figure in the literature is competition MATH; coding reasoning here is unmeasured).
pub fn think_tokens_for_preset(tier: Tier, preset: ThinkPreset) -> u32 {
    use ThinkPreset::*;
    match (tier, preset) {
        (Tier::Easy, Lean) => 1024,
        (Tier::Easy, Standard) => 2048,
        (Tier::Easy, Deep) => 4096,
        (Tier::Medium, Lean) => 2048,
        (Tier::Medium, Standard) => 6144,
        (Tier::Medium, Deep) => 12288,
        (Tier::Hard, Lean) => 4096,
        (Tier::Hard, Standard) => 10240,
        (Tier::Hard, Deep) => 20480,
        (Tier::Extreme, Lean) => 6144,
        (Tier::Extreme, Standard) => 16384,
        (Tier::Extreme, Deep) => 32768,
    }
}

/// Back-compat convenience: the `Standard` preset's scratchpad budget.
pub fn think_tokens_for(tier: Tier) -> u32 {
    think_tokens_for_preset(tier, ThinkPreset::Standard)
}

/// Per-turn output cap for a chosen preset = answer floor + (scratchpad iff the model reasons).
pub fn max_tokens_for_preset(tier: Tier, is_thinking: bool, preset: ThinkPreset) -> u32 {
    answer_tokens_for(tier) + if is_thinking { think_tokens_for_preset(tier, preset) } else { 0 }
}

/// Back-compat convenience: the `Standard` preset. Existing non-D8 call sites keep this signature.
pub fn max_tokens_for(tier: Tier, is_thinking: bool) -> u32 {
    max_tokens_for_preset(tier, is_thinking, ThinkPreset::Standard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn k_scales_monotonically_and_easy_matches_the_legacy_default() {
        assert_eq!(pass_k_for(Tier::Easy), 5); // == pre-Phase-9 AgenticConfig default
        assert_eq!(pass_k_for(Tier::Medium), 8);
        assert_eq!(pass_k_for(Tier::Hard), 16);
        assert_eq!(pass_k_for(Tier::Extreme), 24);
        assert!(pass_k_for(Tier::Easy) < pass_k_for(Tier::Extreme));
    }

    #[test]
    fn max_steps_scales_monotonically_and_easy_matches_the_legacy_ui_default() {
        assert_eq!(max_steps_for(Tier::Easy), 8); // == the pre-Phase-9 UI default
        assert_eq!(max_steps_for(Tier::Medium), 16);
        assert_eq!(max_steps_for(Tier::Hard), 32);
        assert_eq!(max_steps_for(Tier::Extreme), 48);
        assert!(max_steps_for(Tier::Easy) < max_steps_for(Tier::Extreme));
    }

    #[test]
    fn non_thinking_budget_is_the_answer_floor_and_clears_a_batched_turn() {
        // The old flat 256 truncated any write_file payload. The floor now carries a BATCHED
        // multi-call turn at every tier, and monotonically more for harder (bigger-batch) tasks.
        assert_eq!(max_tokens_for(Tier::Easy, false), 1536);
        assert_eq!(max_tokens_for(Tier::Medium, false), 2048);
        assert_eq!(max_tokens_for(Tier::Hard, false), 2560);
        assert_eq!(max_tokens_for(Tier::Extreme, false), 3072);
        // Every non-thinking budget clears a TYPICAL BATCHED turn (~3-file batch), not just a
        // single-file write — the forensic case is multifile/batched, so single-file is the
        // wrong maximum. 1536 is the ~3-file batch estimate; below it, batched turns thrash.
        for tier in [Tier::Easy, Tier::Medium, Tier::Hard, Tier::Extreme] {
            assert!(max_tokens_for(tier, false) >= 1536);
        }
        assert!(max_tokens_for(Tier::Easy, false) <= max_tokens_for(Tier::Extreme, false));
    }

    #[test]
    fn thinking_budget_is_answer_plus_scratchpad_and_scales_monotonically() {
        // Thinking = answer floor + FIXED reasoning scratchpad at the STANDARD preset (same on
        // every machine). Medium's scratchpad clears the observed 3200–3576 reasoning range with
        // answer room to spare; harder tiers reason longer.
        assert_eq!(max_tokens_for(Tier::Easy, true), 1536 + 2048);
        assert_eq!(max_tokens_for(Tier::Medium, true), 2048 + 6144);
        assert_eq!(max_tokens_for(Tier::Hard, true), 2560 + 10240);
        assert_eq!(max_tokens_for(Tier::Extreme, true), 3072 + 16384);
        // Medium's scratchpad is clearly above the observed max reasoning (3576), not a tight fit.
        assert!(think_tokens_for(Tier::Medium) > 3576 + 512, "Medium scratchpad clears observed reasoning + margin");
        assert!(max_tokens_for(Tier::Easy, true) < max_tokens_for(Tier::Extreme, true));
        // Thinking is strictly more generous than the answer-only floor at every tier, and the
        // difference is exactly the scratchpad budget (the separate-budgeting invariant).
        for tier in [Tier::Easy, Tier::Medium, Tier::Hard, Tier::Extreme] {
            assert!(max_tokens_for(tier, true) > max_tokens_for(tier, false));
            assert_eq!(
                max_tokens_for(tier, true) - max_tokens_for(tier, false),
                think_tokens_for(tier)
            );
        }
    }

    #[test]
    fn presets_are_ordered_lean_lt_standard_lt_deep_and_default_is_standard() {
        assert_eq!(ThinkPreset::default(), ThinkPreset::Standard);
        for tier in [Tier::Easy, Tier::Medium, Tier::Hard, Tier::Extreme] {
            let lean = think_tokens_for_preset(tier, ThinkPreset::Lean);
            let std = think_tokens_for_preset(tier, ThinkPreset::Standard);
            let deep = think_tokens_for_preset(tier, ThinkPreset::Deep);
            assert!(lean < std && std < deep, "{tier:?}: Lean {lean} < Standard {std} < Deep {deep}");
        }
        // The bare `think_tokens_for` is the Standard preset (back-compat).
        assert_eq!(think_tokens_for(Tier::Medium), think_tokens_for_preset(Tier::Medium, ThinkPreset::Standard));
        // Non-thinking is preset-independent (only the scratchpad varies by preset).
        assert_eq!(
            max_tokens_for_preset(Tier::Hard, false, ThinkPreset::Deep),
            max_tokens_for_preset(Tier::Hard, false, ThinkPreset::Lean)
        );
    }
}
