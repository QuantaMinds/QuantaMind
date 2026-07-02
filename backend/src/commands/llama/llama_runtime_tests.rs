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
