use super::*;

/// A plain full-precision plan at `ctx` — the roomy-machine path (no forced flags, no note).
fn plain(ctx: u32) -> LaunchPlan {
    LaunchPlan { ctx, flash_attn: false, kv: KvType::F16, note: None }
}

#[test]
fn spawn_args_pass_model_path_host_port_jinja_and_context() {
    let args = build_spawn_args("/models/foo.gguf", 8081, &plain(8192), None);
    assert_eq!(
        args,
        vec![
            "-m",
            "/models/foo.gguf",
            "--host",
            "127.0.0.1",
            "--port",
            "8081",
            "--jinja",
            "-c",
            "8192"
        ]
    );
}

#[test]
fn spawn_args_reflect_a_custom_port() {
    let args = build_spawn_args("/m/x.gguf", 9090, &plain(4096), None);
    assert!(args.windows(2).any(|w| w == ["--port", "9090"]));
}

/// `--jinja` is what makes the chat endpoint apply the model's embedded
/// template; its absence is the loop bug, so guard it explicitly.
#[test]
fn spawn_args_always_include_jinja() {
    let args = build_spawn_args("/m/x.gguf", 8081, &plain(4096), None);
    assert!(args.iter().any(|a| a == "--jinja"));
}

/// A resolved override file is appended as `--chat-template-file <path>`; absent,
/// no such flag (the embedded template via `--jinja` is the default).
#[test]
fn spawn_args_append_chat_template_file_only_when_present() {
    let with = build_spawn_args("/m/x.gguf", 8081, &plain(4096), Some("/cfg/chat_templates/gemma.jinja"));
    assert!(with.windows(2).any(|w| w == ["--chat-template-file", "/cfg/chat_templates/gemma.jinja"]));
    let without = build_spawn_args("/m/x.gguf", 8081, &plain(4096), None);
    assert!(!without.iter().any(|a| a == "--chat-template-file"));
}

/// The roomy-machine plan adds NO memory flags — byte-identical to the legacy launch.
#[test]
fn spawn_args_omit_memory_flags_on_a_plain_plan() {
    let args = build_spawn_args("/m/x.gguf", 8081, &plain(8192), None);
    assert!(!args.iter().any(|a| a == "-fa"), "no flash-attn flag on a roomy host");
    assert!(!args.iter().any(|a| a == "-ctk"), "no quantized-KV flag on a roomy host");
}

/// A memory-constrained plan emits flash attention AND the Q8 KV-cache flags together
/// (a Q8 cache requires flash attention) — the exact flags that avert the Metal OOM wedge.
#[test]
fn spawn_args_emit_flash_attn_and_q8_kv_on_a_constrained_plan() {
    let plan = LaunchPlan { ctx: 16_384, flash_attn: true, kv: KvType::Q8, note: Some("tight".into()) };
    let args = build_spawn_args("/m/x.gguf", 8081, &plan, None);
    assert!(args.windows(2).any(|w| w == ["-fa", "on"]), "flash attention forced on");
    assert!(args.windows(2).any(|w| w == ["-ctk", "q8_0"]), "K cache quantized");
    assert!(args.windows(2).any(|w| w == ["-ctv", "q8_0"]), "V cache quantized");
    assert!(args.windows(2).any(|w| w == ["-c", "16384"]));
}

/// KV dims of a ~9B model: enough per-token cost that f16 can't hold a 16K window on 16 GB,
/// so the plan must fall back to flash-attn + Q8 KV and say so.
fn nineb_dims() -> KvDims {
    KvDims { layers: 36, head_count: 40, head_count_kv: 8, embedding_length: 5120 }
}

/// Roomy host (128 GB): the desired window fits at full precision → plain plan, no note.
#[test]
fn plan_launch_leaves_a_roomy_host_untouched() {
    let plan = plan_launch(Some(9_000_000_000), Some(nineb_dims()), 128 * 1_000_000_000, Some(32_768), Some(16_384));
    assert_eq!(plan.kv, KvType::F16);
    assert!(!plan.flash_attn);
    assert_eq!(plan.ctx, 16_384);
    assert!(plan.note.is_none(), "no constraint → no user message");
}

/// Tight host (16 GB) asked for a big window a 9B can't hold at f16 → flash-attn + Q8 KV,
/// and a note that names the safe config so the UI can show it.
#[test]
fn plan_launch_engages_flash_attn_and_q8_and_notifies_on_a_tight_host() {
    let plan = plan_launch(Some(9_000_000_000), Some(nineb_dims()), 16 * 1_000_000_000, Some(32_768), Some(16_384));
    assert_eq!(plan.kv, KvType::Q8, "quantize the KV cache to fit");
    assert!(plan.flash_attn, "Q8 KV requires flash attention");
    assert!(plan.ctx >= MIN_CONTEXT && plan.ctx <= 16_384);
    let note = plan.note.expect("a constraint was applied → the user must be told");
    assert!(note.contains("Flash Attention") && note.contains("Q8"), "note names the safe config: {note}");
    assert!(note.to_lowercase().contains("safely"), "note frames it as running safely: {note}");
}

/// Pinned EXACT plan for the tight-host fixture — the tripwire that the
/// `KvPrecision`-based Q8 math is bit-identical to the former `per_token / 2`:
/// per_token_f16 = 2×36×8×128×2 = 147,456 B; usable = 70% of 16 GB = 11.2 GB;
/// budget = 2.2 GB → f16 ceiling 14,848 < 16,384 desired → Q8 (73,728 B/token)
/// ceiling 29,696 holds it → ctx stays exactly 16,384, second note branch.
#[test]
fn plan_launch_q8_ceiling_unchanged_by_precision_refactor() {
    let plan = plan_launch(Some(9_000_000_000), Some(nineb_dims()), 16 * 1_000_000_000, Some(32_768), Some(16_384));
    assert_eq!(plan.ctx, 16_384);
    assert_eq!(plan.kv, KvType::Q8);
    assert!(plan.flash_attn);
    assert_eq!(
        plan.note.as_deref(),
        Some(
            "Detected 16 GB of RAM. Running safely: enabled Flash Attention and a Q8 KV cache \
             (half the memory) so the 16384-token context fits without a GPU out-of-memory error."
        )
    );

    // Even tighter host (10 GB): budget saturates to 0 → both ceilings floor at
    // MIN_CONTEXT → capped-context branch, exact ctx pinned.
    let capped = plan_launch(Some(9_000_000_000), Some(nineb_dims()), 10 * 1_000_000_000, Some(32_768), Some(16_384));
    assert_eq!(capped.ctx, MIN_CONTEXT);
    assert_eq!(capped.kv, KvType::Q8);
    assert!(capped.note.as_deref().unwrap().contains("capped the context to 2048"));
}

/// `KvType` → `KvPrecision` is total over the launch domain: no Q4 arm exists,
/// which is the type-level proof a launch can never auto-pick a Q4 cache.
#[test]
fn kv_type_maps_to_precision() {
    use crate::inference::vram_math::KvPrecision;
    assert_eq!(KvType::F16.precision(), KvPrecision::F16);
    assert_eq!(KvType::Q8.precision(), KvPrecision::Q8);
}

/// The three ceilings grow as the cache shrinks, each snapped to CTX_STEP.
/// Hand-checked on the 9B fixture at 16 GB: per_token_f16 = 147,456 B; usable =
/// 70% × 16 GB = 11.2 GB; budget = 2.2 GB → f16 = 14,848; q8 (73,728 B) → 29,696;
/// q4 (36,864 B) → 59,648. Q8 doubles F16 here; Q4 is ~4× (4.017×) — NOT exact,
/// because each ceiling is independently floored to CTX_STEP, so the meters must
/// show the real stepped values, never a naive "×2/×4".
#[test]
fn ctx_ceilings_q8_roughly_doubles_and_q4_quadruples_f16() {
    let c = ctx_ceilings(9_000_000_000, nineb_dims(), 16 * 1_000_000_000, None);
    assert_eq!(c.f16, Some(14_848));
    assert_eq!(c.q8, Some(29_696));
    assert_eq!(c.q4, Some(59_648));
    let (f16, q8, q4) = (c.f16.unwrap(), c.q8.unwrap(), c.q4.unwrap());
    assert!(f16 < q8 && q8 < q4, "more context as the cache shrinks");
    assert_eq!(q8, f16 * 2, "q8 halves the per-token cost → exactly 2× here");
    // Q4 is APPROXIMATELY 4× (independent stepping), never asserted as exact.
    assert!((q4 as f64 / f16 as f64 - 4.0).abs() < 0.1, "q4 ≈ 4× f16: {}", q4 as f64 / f16 as f64);
}

/// Zero per-token cost (embedding_length 0 → head_dim 0) is unmeasurable → every
/// precision is `None`, never a fabricated ceiling.
#[test]
fn ctx_ceilings_unmeasurable_dims_yield_none_never_a_clamp() {
    let bad = KvDims { layers: 36, head_count: 40, head_count_kv: 8, embedding_length: 0 };
    let c = ctx_ceilings(9_000_000_000, bad, 16 * 1_000_000_000, None);
    // 9 GB weights below the 11.2 GB (70% of 16 GB) heuristic budget, but the limit is
    // unmeasured here (None) → fit is Unknown, never guessed from the heuristic.
    assert_eq!(c, CtxCeilings { f16: None, q8: None, q4: None, fit: FitVerdict::Unknown });
}

/// The fit verdict classifies the WEIGHTS against the measured GPU limit — the ceilings
/// can't. Below 85% → Fits; ≥85% but under → Tight; at/over → SpillsToCpu; unmeasured or a
/// zero model size → Unknown (never guessed).
#[test]
fn fit_verdict_classifies_weights_against_the_gpu_limit() {
    let limit = 12_000_000_000; // ~ a 16 GB Mac's Metal working set
    assert_eq!(fit_verdict(5_000_000_000, Some(limit)), FitVerdict::Fits); // 42% — room for KV
    assert_eq!(fit_verdict(11_000_000_000, Some(limit)), FitVerdict::Tight); // 92% — fits, no KV room
    assert_eq!(fit_verdict(13_000_000_000, Some(limit)), FitVerdict::SpillsToCpu); // over the limit
    assert_eq!(fit_verdict(5_000_000_000, None), FitVerdict::Unknown); // limit unmeasured
    assert_eq!(fit_verdict(0, Some(limit)), FitVerdict::Unknown); // no model size
}

/// The 25 GB-model-on-a-32 GB-Mac trap: weights exceed the Metal working set (~24 GB), so the
/// model spills to CPU no matter how large the raw context ceiling looks — the whole reason the
/// verdict exists alongside the ceilings.
#[test]
fn ctx_ceilings_flags_the_oversized_model_that_spills_to_cpu() {
    let working_set = 24_000_000_000; // ~75% of a 32 GB Mac
    let c = ctx_ceilings(25_000_000_000, nineb_dims(), 32_000_000_000, Some(working_set));
    assert_eq!(c.fit, FitVerdict::SpillsToCpu);
}

/// Budgeting against the MEASURED working set, not total×70%: a working set below the heuristic
/// yields a smaller (more honest) ceiling than the legacy total-only path — proving the measured
/// cap actually drives the budget.
#[test]
fn ctx_ceilings_budget_follows_the_measured_working_set() {
    let (total, weights) = (16_000_000_000u64, 4_000_000_000u64);
    let heuristic = ctx_ceilings(weights, nineb_dims(), total, None); // 70% of 16 GB = 11.2 GB
    let measured = ctx_ceilings(weights, nineb_dims(), total, Some(10_000_000_000)); // smaller real cap
    assert!(
        measured.f16.unwrap() < heuristic.f16.unwrap(),
        "a smaller measured cap must yield a smaller ceiling: {:?} vs {:?}",
        measured.f16,
        heuristic.f16
    );
    assert_eq!(heuristic.fit, FitVerdict::Unknown, "no measured limit → no fit verdict");
    assert_eq!(measured.fit, FitVerdict::Fits, "4 GB weights well under a 10 GB limit");
}

/// Unmeasurable dims → we can't budget memory, so never fabricate a constraint: plain plan.
#[test]
fn plan_launch_is_plain_when_dims_are_unknown() {
    let plan = plan_launch(Some(9_000_000_000), None, 16 * 1_000_000_000, Some(32_768), Some(16_384));
    assert_eq!(plan.kv, KvType::F16);
    assert!(plan.note.is_none());
}

/// The `-c` value must be CAPPED: a GGUF's declared context is the model MAX
/// (gemma4 = 262144), and `-c 262144` OOMs the KV cache → llama-server "Compute
/// error". Cap to MAX_CONTEXT; never exceed the model's own value; floor when absent.
#[test]
fn context_is_capped_to_avoid_kv_oom() {
    assert_eq!(cap_context(Some(262_144)), MAX_CONTEXT, "256K must cap, not OOM");
    assert_eq!(cap_context(Some(2048)), 2048, "a small model context is kept as-is");
    assert_eq!(cap_context(None), DEFAULT_CONTEXT, "missing header → floor");
    assert!(MAX_CONTEXT >= DEFAULT_CONTEXT, "cap gives headroom above the old default");
}

/// A ceiling far above any tested value — isolates the gguf/requested logic from the
/// hardware clamp (which has its own tests).
const NO_HW_LIMIT: u32 = u32::MAX;

/// `resolve_launch_ctx` is the "use params" path: the user's `num_ctx` drives the
/// launch `-c` (llama.cpp can't change context per request), bounded by the model
/// max; unset falls back to the safe default cap.
#[test]
fn resolve_launch_ctx_honors_user_num_ctx_bounded_by_model_max() {
    // User opts in past the 8192 default — honored (informed; tooltip warns re memory).
    assert_eq!(resolve_launch_ctx(Some(32_768), Some(16_384), NO_HW_LIMIT), 16_384, "user value above the default cap is honored");
    // …but never beyond the model's declared max.
    assert_eq!(resolve_launch_ctx(Some(8_192), Some(40_000), NO_HW_LIMIT), 8_192, "user value clamps to the model max");
    // Unknown model max: trust the user's explicit value.
    assert_eq!(resolve_launch_ctx(None, Some(12_000), NO_HW_LIMIT), 12_000, "no header → honor the requested value");
}

#[test]
fn resolve_launch_ctx_falls_back_to_default_cap_when_unset() {
    // Empty/zero param → the old safe default (GGUF value capped at MAX_CONTEXT).
    assert_eq!(resolve_launch_ctx(Some(262_144), None, NO_HW_LIMIT), MAX_CONTEXT, "unset → 256K caps, not OOM");
    assert_eq!(resolve_launch_ctx(Some(262_144), Some(0), NO_HW_LIMIT), MAX_CONTEXT, "0 is treated as unset");
    assert_eq!(resolve_launch_ctx(Some(4_096), None, NO_HW_LIMIT), 4_096, "unset → small model context kept as-is");
    assert_eq!(resolve_launch_ctx(None, None, NO_HW_LIMIT), DEFAULT_CONTEXT, "no header, unset → floor");
}

/// The hardware ceiling clamps BOTH paths (explicit and default) so even a high
/// `num_ctx` can't OOM the pre-allocated KV cache; the result never drops below the
/// `MIN_CONTEXT` floor.
#[test]
fn resolve_launch_ctx_clamps_to_hardware_ceiling() {
    // Explicit 32K on a machine that holds ~12K → clamped to 12K, not the model max.
    assert_eq!(resolve_launch_ctx(Some(262_144), Some(32_768), 12_000), 12_000, "explicit value clamps to the hardware ceiling");
    // The model max still wins when it's below the ceiling.
    assert_eq!(resolve_launch_ctx(Some(4_096), Some(32_768), 12_000), 4_096, "model max below the ceiling still caps");
    // The default (unset) path is clamped too.
    assert_eq!(resolve_launch_ctx(Some(262_144), None, 6_000), 6_000, "unset default path is also RAM-bounded");
    // Never below the floor, even with an absurdly tight ceiling.
    assert_eq!(resolve_launch_ctx(Some(262_144), Some(32_768), 100), MIN_CONTEXT, "result is floored at MIN_CONTEXT");
}

/// Regression (gemma-4-12b): a model whose GGUF dims don't fully parse — gemma stores
/// `attention.head_count_kv` as a per-layer array, so `dims = None` — must NOT have its
/// explicit `num_ctx` clamped. The unmeasurable ceiling is `u32::MAX`, so the user's
/// window is honored (up to the model max) and only the unset default still caps at 8K.
#[test]
fn unmeasurable_ceiling_never_clamps_an_explicit_window() {
    let ceiling = hardware_ctx_ceiling(7_000_000_000, None, 16 * 1024 * 1024 * 1024);
    assert_eq!(ceiling, u32::MAX, "no dims → no measurable ceiling");
    assert_eq!(resolve_launch_ctx(Some(262_144), Some(16_384), ceiling), 16_384, "explicit window honored, not clamped to 8K");
    assert_eq!(resolve_launch_ctx(Some(262_144), None, ceiling), MAX_CONTEXT, "unset still caps at the 8K default");
}

/// `hardware_ctx_ceiling` is conservative: it budgets only `USABLE_MEMORY_PCT` of
/// TOTAL RAM and reserves the weights, so the launched `-c` stays well under the naive
/// `total / per-token`, and degrades to the safe default when dims are missing rather
/// than dividing by zero. Budgeting on total (not free) memory keeps the ceiling stable
/// regardless of current system load.
#[test]
fn hardware_ctx_ceiling_is_conservative_and_degrades_safely() {
    // Llama-3-8B (GQA) dims: 32 layers, 32 heads, 8 KV heads, 4096 emb → 128 KiB/token.
    let dims = KvDims { layers: 32, head_count: 32, head_count_kv: 8, embedding_length: 4096 };
    let per_token = 2 * 32 * 8 * (4096 / 32) * 2; // 131_072 bytes/token
    let model_bytes = 4_600u64 * 1024 * 1024; // ~4.6 GiB Q4 weights
    let total = 16u64 * 1024 * 1024 * 1024; // 16 GiB machine
    let ceiling = hardware_ctx_ceiling(model_bytes, Some(dims), total);

    // Below the naive (full-RAM, no-weights) ceiling — the usable fraction and weight
    // reservation were applied.
    let naive = (total / per_token) as u32;
    assert!(ceiling < naive, "usable fraction + weights must pull the ceiling below the naive {naive}, got {ceiling}");
    // A 16 GiB machine running a 4.6 GiB 7B comfortably clears the cliff's ~18K need.
    assert!(ceiling >= 18_432, "16 GiB must hold the deepest cliff rung for a 7B, got {ceiling}");
    assert_eq!(ceiling % 256, 0, "rounded down to a tidy step");

    // Missing dims → NO clamp (u32::MAX), never a bogus cap that would defeat an
    // explicit `num_ctx`. The unset-default 8K cap lives in `cap_context`, not here.
    assert_eq!(hardware_ctx_ceiling(model_bytes, None, total), u32::MAX, "absent dims → no RAM clamp");
    // A machine too small for even the weights → floored, never zero.
    assert_eq!(hardware_ctx_ceiling(model_bytes, Some(dims), 1024), MIN_CONTEXT, "no usable budget → MIN_CONTEXT floor");
}

#[test]
fn jinja_unsupported_detects_rejected_flag_signature() {
    let mut tail = std::collections::VecDeque::new();
    tail.push_back("error: invalid argument: --jinja".to_string());
    assert!(jinja_unsupported(&tail));

    let mut other = std::collections::VecDeque::new();
    other.push_back("error while handling argument \"--jinja\"".to_string());
    assert!(jinja_unsupported(&other));
}

#[test]
fn jinja_unsupported_ignores_benign_stderr() {
    let mut tail = std::collections::VecDeque::new();
    tail.push_back("llama_model_loader: loaded meta data".to_string());
    tail.push_back("main: server listening on 127.0.0.1:8081".to_string());
    assert!(!jinja_unsupported(&tail));
}

/// With no sidecar listening on 8081, the health probe reports unavailable (not an
/// error) — the shape the frontend poll + batch pre-flight rely on.
#[tokio::test]
async fn health_reports_unavailable_when_no_server_is_running() {
    let h = check_llama_health().await;
    assert!(!h.available);
    assert_eq!(h.version, None);
}

/// The real `/props` body an externally-started llama-server returns (captured live from a
/// `qm`-launched gpt-oss-20b) → the loaded model's absolute path and per-slot context window,
/// so the Inspector can surface a server the app didn't spawn.
#[test]
fn parse_props_reads_model_path_and_ctx() {
    let body = r#"{"default_generation_settings":{"params":{"seed":42},"n_ctx":16384},"total_slots":4,"model_alias":"gpt-oss-20b_q4_k_m.gguf","model_path":"/Users/x/.quantamind/gguf/gpt-oss-20b_q4_k_m.gguf"}"#;
    assert_eq!(
        parse_props(body),
        Some(("/Users/x/.quantamind/gguf/gpt-oss-20b_q4_k_m.gguf".to_string(), 16384))
    );
}

/// Older llama.cpp omits top-level `model_path`, carrying the path in
/// `default_generation_settings.model` — the fallback picks it up.
#[test]
fn parse_props_falls_back_to_generation_settings_model() {
    let body = r#"{"default_generation_settings":{"model":"/models/foo.gguf","n_ctx":4096}}"#;
    assert_eq!(parse_props(body), Some(("/models/foo.gguf".to_string(), 4096)));
}

/// No model path anywhere, and non-JSON, both degrade to None — never a fabricated entry.
#[test]
fn parse_props_returns_none_without_a_path() {
    assert_eq!(parse_props(r#"{"default_generation_settings":{"n_ctx":8192}}"#), None);
    assert_eq!(parse_props("not json"), None);
}


