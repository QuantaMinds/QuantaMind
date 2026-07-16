use super::profile::ReadinessProfile;
use super::types::{CliffStatus, NativeFcStatus, Readiness, ReadinessInputs, ReadinessVerdict, EPSILON};
use crate::inference::eval::agentic::spec::Tier;

/// "" for a count of 1, "s" otherwise — so a reason reads "1 run" not "1 runs".
fn plural(n: u32) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// Pure synthesis: measured inputs + a profile → a verdict. No async, no I/O —
/// the single source of truth shared by the GUI command and the future CLI.
/// Hard gates push to `blocking` (→ NotReady); soft targets push to `conditions`
/// (→ Conditional). A required-but-unmeasured metric blocks: ignorance is not a pass.
pub fn assess(i: &ReadinessInputs, p: &ReadinessProfile) -> ReadinessVerdict {
    let mut blocking = Vec::new();
    let mut conditions = Vec::new();

    // Core hard gate: Pass^k must be measured and meet the bar (epsilon-guarded).
    match i.pass_k {
        None => blocking.push("pass^k not measured (no agentic runs) — cannot certify".into()),
        Some(pk) if pk < p.min_pass_k - EPSILON => {
            blocking.push(format!("pass^k {:.2} < {:.2} required", pk, p.min_pass_k));
        }
        Some(_) => {}
    }

    // Failure-taxonomy hard gates (counts are always known when agentic ran, so report
    // the exact number of affected runs — never a vague "some runs").
    if p.forbid_infinite_loop && i.loops > 0 {
        blocking.push(format!("loops on {} run{}", i.loops, plural(i.loops)));
    }
    if p.forbid_hallucinated_completion && i.hallucinated > 0 {
        blocking.push(format!("false 'done' on {} run{}", i.hallucinated, plural(i.hallucinated)));
    }

    // Hardware gate. A MEASURED bad fit is a hard block; an UNMEASURED fit is an honest
    // caveat (Conditional), not a red failure — "unmeasured ≠ guessed fail," consistent
    // with the Tier Matrix's gray NOT-TESTED. (On llama.cpp / Apple-Silicon unified memory
    // the fit often can't be measured, so blocking would falsely red-flag every model.)
    // "memory" not "VRAM" — unified-memory machines have no discrete VRAM.
    if p.require_full_vram {
        match i.fits_in_vram {
            Some(false) => blocking.push("partial offload → severe slowdown".into()),
            None => conditions.push("memory fit not measured — set a memory cap to certify".into()),
            Some(true) => {}
        }
    }
    // VRAM pressure: fits but near the allocation ceiling → soft Conditional note.
    // Independent of require_full_vram — even an offload-tolerant profile wants the
    // heads-up that headroom is thin.
    if i.vram_pressure {
        conditions.push("high VRAM pressure near allocation ceiling".into());
    }
    // Gate-at-actual-KV transparency: when the fit was graded at the Q8 cache the
    // llama.cpp launch would actually use, the verdict SAYS so — always, even
    // alongside a blocking bad fit — so a Q8-graded row can never be silently
    // compared against an f16-graded one.
    if i.kv_downgraded {
        conditions.push("fits with Q8 KV cache — ≈half cache memory, minor quality cost".into());
    }

    // Context-cliff hard gate (only when the profile demands headroom). Strict: a
    // NoCliff passes only if the probe actually reached the required depth — an
    // incomplete probe (or an unprobed model) is not a pass.
    if let Some(min_tok) = p.min_context_tokens {
        match i.cliff {
            CliffStatus::Collapsed { depth } if depth < min_tok => {
                blocking.push(format!("reasoning cliff at {} < {} needed", depth, min_tok));
            }
            CliffStatus::NoCliff { tested } if tested < min_tok => {
                blocking.push(format!("only probed to {} tok < {} needed (no cliff, but headroom unproven)", tested, min_tok));
            }
            CliffStatus::Broken { .. } => {
                blocking.push("tool-call accuracy fails at the baseline (broken) — no usable context window".to_string());
            }
            CliffStatus::NotProbed => {
                // Unmeasured ≠ failure: a caveat to run the probe, not a red block.
                conditions.push(format!("context headroom not measured — run the cliff probe to certify {} tok", min_tok));
            }
            CliffStatus::Inconclusive { trials } => {
                // The probe RAN but its collection is too small to resolve the collapse
                // margin, so its verdict carries no information. Same rule as NotProbed —
                // unmeasured ≠ failure — but it must be stated explicitly: the `_` arm below
                // would silently read it as a PASS, certifying headroom nothing measured.
                conditions.push(format!(
                    "context headroom inconclusive — {} samples/rung can't resolve the collapse margin; probe a larger collection to certify {} tok",
                    trials, min_tok
                ));
            }
            _ => {} // Collapsed{depth >= min} or NoCliff{tested >= min} → pass
        }
    }

    // Soft targets → Conditional on breach only, and silent when unmeasured: an
    // advisory target we didn't run shouldn't downgrade an otherwise-clean model.
    // (Hard gates only block on a MEASURED failure; their unmeasured case is a
    // Conditional caveat, above — never a red block.)
    if let (Some(mx), Some(ms)) = (p.max_ms_per_step, i.ms_per_step) {
        if ms > mx {
            conditions.push(format!("slow: {}ms/step > {}ms target", ms, mx));
        }
    }
    if let (Some(mx), Some(s)) = (p.max_avg_steps, i.avg_steps) {
        if s > mx {
            conditions.push(format!("inefficient: {:.1} avg steps > {:.1} max", s, mx));
        }
    }

    // Native-FC: hard gate only when the profile requires it. Otherwise the
    // `path` label below carries the "prompt-based proxy" transparency — we do
    // NOT force every prompt-based model to Conditional (it would make a clean
    // model uncertifiable until 7.2 lands).
    if matches!(i.native_fc, NativeFcStatus::NotSupported) && p.require_native_fc {
        blocking.push("native tool-calling required but not supported/measured on this backend".into());
    }

    // Phase 9 hard gate (hardware-calibrated difficulty): the model must clear the
    // profile's required tier. `cleared_tier` is the highest tier cleared at this
    // profile's bar (computed once, also surfaced in the report). The gate blocks
    // ONLY when the collection actually exercised the required tier — an untested
    // tier is NotAttempted, never a guessed fail, so an all-Easy collection never
    // trips a Hard profile. A pre-Phase-9 profile defaults `required_tier = Easy`,
    // which this never blocks on (exact old behavior).
    let cleared_tier =
        i.tier_pass_k.iter().filter(|(_, pk)| *pk >= p.min_pass_k - EPSILON).map(|(t, _)| *t).max();
    if p.required_tier > Tier::Easy {
        let exercised = i.tier_pass_k.iter().any(|(t, _)| *t >= p.required_tier);
        if exercised && cleared_tier.map_or(true, |c| c < p.required_tier) {
            blocking.push(match cleared_tier {
                Some(c) => format!("cleared {c:?}; this profile requires {:?}", p.required_tier),
                None => format!("cleared no tier at pass^k {:.2}; requires {:?}", p.min_pass_k, p.required_tier),
            });
        }
    }

    // The path is the row's OWN path, set explicitly by the caller — NOT derived from
    // `native_fc` (which now carries the model-level capability shared by both rows). This
    // is what lets a native-capable model's prompt-based row label itself `PromptBased`
    // while still passing the `require_native_fc` gate above (its `native_fc` is `Tested`).
    let path = i.path;
    let status = if !blocking.is_empty() {
        Readiness::NotReady
    } else if !conditions.is_empty() {
        Readiness::Conditional
    } else {
        Readiness::Ready
    };
    ReadinessVerdict { status, blocking, conditions, path, required_tier: p.required_tier, cleared_tier }
}

#[cfg(test)]
#[path = "verdict_tests.rs"]
mod tests;
