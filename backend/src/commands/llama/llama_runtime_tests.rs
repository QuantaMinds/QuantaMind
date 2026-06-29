use super::*;

#[test]
fn spawn_args_pass_model_path_host_port_jinja_and_context() {
    let args = build_spawn_args("/models/foo.gguf", 8081, 8192, None);
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
    let args = build_spawn_args("/m/x.gguf", 9090, 4096, None);
    assert!(args.windows(2).any(|w| w == ["--port", "9090"]));
}

/// `--jinja` is what makes the chat endpoint apply the model's embedded
/// template; its absence is the loop bug, so guard it explicitly.
#[test]
fn spawn_args_always_include_jinja() {
    let args = build_spawn_args("/m/x.gguf", 8081, 4096, None);
    assert!(args.iter().any(|a| a == "--jinja"));
}

/// A resolved override file is appended as `--chat-template-file <path>`; absent,
/// no such flag (the embedded template via `--jinja` is the default).
#[test]
fn spawn_args_append_chat_template_file_only_when_present() {
    let with = build_spawn_args("/m/x.gguf", 8081, 4096, Some("/cfg/chat_templates/gemma.jinja"));
    assert!(with.windows(2).any(|w| w == ["--chat-template-file", "/cfg/chat_templates/gemma.jinja"]));
    let without = build_spawn_args("/m/x.gguf", 8081, 4096, None);
    assert!(!without.iter().any(|a| a == "--chat-template-file"));
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

    // Missing dims → the safe default cap, never a divide-by-zero or over-allocation.
    assert_eq!(hardware_ctx_ceiling(model_bytes, None, total), MAX_CONTEXT, "absent dims fall back to the default cap");
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
