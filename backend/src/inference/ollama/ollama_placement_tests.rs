use super::*;

fn ps_body(size: u64, size_vram: u64) -> String {
    format!(r#"{{"models":[{{"name":"qwen3.5:9b","model":"qwen3.5:9b","size":{size},"size_vram":{size_vram}}}]}}"#)
}

/// Fully resident (vram == size): not on CPU, no note.
#[test]
fn fully_resident_model_is_not_on_cpu() {
    let p = parse_placement(&ps_body(9_000_000_000, 9_000_000_000), "qwen3.5:9b").unwrap();
    assert!(!p.on_cpu);
    assert_eq!(p.cpu_bytes, 0);
    assert!(p.note().is_none(), "a resident model needs no message");
}

/// A tiny rounding difference (<5% on CPU) must NOT flag as running on CPU.
#[test]
fn a_few_percent_rounding_is_not_flagged_as_cpu() {
    // 2% on CPU.
    let p = parse_placement(&ps_body(10_000_000_000, 9_800_000_000), "qwen3.5:9b").unwrap();
    assert!(!p.on_cpu, "2% spill is rounding, not offload");
}

/// A meaningful partial spill (>5% on CPU) flags on_cpu and produces a "partly on CPU" note.
#[test]
fn a_meaningful_partial_spill_is_on_cpu_with_a_note() {
    // 40% on CPU.
    let p = parse_placement(&ps_body(10_000_000_000, 6_000_000_000), "qwen3.5:9b").unwrap();
    assert!(p.on_cpu);
    assert_eq!(p.cpu_bytes, 4_000_000_000);
    let note = p.note().expect("an offloaded model must tell the user");
    assert!(note.contains("partly on CPU"), "note names the partial spill: {note}");
    assert!(note.to_lowercase().contains("more time"), "note explains the eval gives more time: {note}");
}

/// Fully on CPU (no VRAM) gets the strongest note.
#[test]
fn fully_on_cpu_gets_the_no_gpu_note() {
    let p = parse_placement(&ps_body(9_000_000_000, 0), "qwen3.5:9b").unwrap();
    assert!(p.on_cpu);
    assert_eq!(p.vram_bytes, 0);
    assert!(p.note().unwrap().contains("no GPU offload"));
}

/// A model that isn't loaded (absent from /api/ps) → None, so the caller degrades gracefully.
#[test]
fn an_unloaded_model_returns_none() {
    assert!(parse_placement(&ps_body(9_000_000_000, 9_000_000_000), "some-other-model").is_none());
    assert!(parse_placement(r#"{"models":[]}"#, "qwen3.5:9b").is_none());
}
