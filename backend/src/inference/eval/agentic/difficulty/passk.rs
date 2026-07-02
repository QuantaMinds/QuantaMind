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

/// The per-turn output-token budget (`num_predict`) is SEPARATELY budgeted (arXiv 2605.07686):
/// a *solution/answer* portion every model gets, plus a *reasoning scratchpad* portion added
/// only for thinking models. The old flat 256 cap for non-thinking models was a structural
/// failure guarantee — a `write_file(content=…)` payload cannot fit in 256 tokens, so the call
/// was truncated mid-string, parsed to zero calls, and scored Malformed/Hallucinated regardless
/// of model SIZE. Token budget is a hidden correctness parameter; sizing it so truncation
/// approaches zero is what lets the eval measure real capability rather than the harness's cap.
/// This is pure I/O plumbing, NOT a difficulty axis: `pass_k_for` / `max_steps_for` / decoys /
/// traps are unchanged, so the tiers stay exactly as hard.

/// The solution/answer budget: enough for a TYPICAL BATCHED turn (several tool calls — e.g.
/// multiple file writes — in one JSON array), NOT a single file. The FLOOR for EVERY model,
/// thinking or not, because the deliverable is identical. A capable agent batches writes in one
/// turn (good behavior); a single-file floor would make batched turns truncate→retry→thrash and
/// could label an efficient batcher `Truncated`. The headroom-clamped retry (see runner.rs)
/// covers the rare oversized batch; this floor keeps the common batched turn off the retry path.
pub fn answer_tokens_for(tier: Tier) -> u32 {
    match tier {
        Tier::Easy => 1536,
        Tier::Medium => 2048,
        Tier::Hard => 2560,
        Tier::Extreme => 3072,
    }
}

/// The reasoning-scratchpad budget, ADDED only for thinking models (the `<think>…</think>`
/// block, stripped before the transcript append so it never accumulates across the horizon).
/// Covers the 1–2k range reasoning models routinely spend, scaling with task complexity. Every
/// resulting total fits inside `agentic_num_ctx` (Hard 14336 / Extreme 16384) as a single turn's
/// generation buffer.
pub fn think_tokens_for(tier: Tier) -> u32 {
    match tier {
        Tier::Easy => 1024,
        Tier::Medium => 1536,
        Tier::Hard => 2048,
        Tier::Extreme => 2048,
    }
}

/// Per-turn output cap = answer floor + (scratchpad iff the model reasons). Non-thinking gets
/// the answer floor alone (1024–2048, up from the fatal 256); thinking gets answer + scratchpad
/// (2048–4096). Call sites (`build.rs` / `batch_cmd.rs`) are unchanged — same signature.
pub fn max_tokens_for(tier: Tier, is_thinking: bool) -> u32 {
    answer_tokens_for(tier) + if is_thinking { think_tokens_for(tier) } else { 0 }
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
        // Thinking = answer floor + reasoning scratchpad. Load-bearing totals; every tier
        // clears the 1–2k scratchpad range PLUS the batched answer, so the call is never truncated.
        assert_eq!(max_tokens_for(Tier::Easy, true), 1536 + 1024);
        assert_eq!(max_tokens_for(Tier::Medium, true), 2048 + 1536);
        assert_eq!(max_tokens_for(Tier::Hard, true), 2560 + 2048);
        assert_eq!(max_tokens_for(Tier::Extreme, true), 3072 + 2048);
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
}
