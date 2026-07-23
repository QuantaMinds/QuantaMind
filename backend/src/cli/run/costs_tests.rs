use super::*;
use crate::inference::eval::agentic::scoring::report::AgenticReport;
use crate::inference::eval::agentic::step::StepKind;

fn step(run: u32, idx: u32) -> TrajectoryStep {
    TrajectoryStep {
        run_index: run,
        step_index: idx,
        raw_output: "{}".into(),
        injection: None,
        kind: StepKind::ToolCall,
        env: Default::default(),
        cache_n: None,
        prefill_tokens: None,
        prefill_ms: None,
        eval_ms: None,
        load_ms: None,
        total_ms: None,
        output_tokens: None,
        resident_bytes: None,
        reasoning_tokens: None,
        thinking_split_measured: false,
        context_used: None,
        context_window: None,
        initial_prompt: None,
    }
}

/// The taskCost.ts mirror: reported fields sum, unreported stay None (never 0), the
/// peak is a single-run maximum, and RSS is a max of step-end samples.
#[test]
fn rows_sum_reported_fields_and_never_fabricate_unreported_ones() {
    let steps = vec![
        TrajectoryStep { prefill_ms: Some(100), eval_ms: Some(900), output_tokens: Some(40), cache_n: Some(0), prefill_tokens: Some(30), resident_bytes: Some(5), ..step(0, 0) },
        TrajectoryStep { prefill_ms: Some(20), eval_ms: Some(800), output_tokens: Some(35), cache_n: Some(60), prefill_tokens: Some(12), resident_bytes: Some(9), ..step(0, 1) },
        step(1, 0), // synthetic terminal — contributes nothing, breaks nothing
    ];
    let row = task_cost_row("t1", false, &steps, None);
    assert_eq!(row.pass, "prompt");
    assert_eq!(row.runs, 2);
    assert_eq!(row.prefill_ms_total, Some(120));
    assert_eq!(row.eval_ms_total, Some(1700));
    assert_eq!(row.output_tokens_total, Some(75));
    assert_eq!(row.cache_hit_tokens_total, Some(60));
    assert_eq!(row.peak_context_tokens, Some(107)); // 60 + 12 + 35
    assert_eq!(row.max_step_end_rss_bytes, Some(9));
    assert_eq!(row.reasoning_tokens_total, None, "nothing reported thinking — must stay None");
    assert!(!row.thinking_split_measured);
    assert!(!row.oom);
}

/// Outcome facts ride the row: measured wall clock from the report; the OOM verdict
/// from the classified error — never re-derived from strings here.
#[test]
fn outcome_carries_wall_and_oom() {
    let ok = TaskOutcome::Agentic { report: AgenticReport::from_outcomes(&[]).with_wall_ms(2777) };
    assert_eq!(task_cost_row("t", false, &[step(0, 0)], Some(&ok)).wall_ms, Some(2777));
    let boom = TaskOutcome::Error { message: "CUDA error: out of memory".into(), oom: true };
    let row = task_cost_row("t", true, &[step(0, 0)], Some(&boom));
    assert!(row.oom);
    assert_eq!(row.pass, "native");
}

/// KV-at-peak needs BOTH dims and a peak; either missing → None, never a guess. When
/// present, the three precisions come from the canonical formula (q8 half, q4 quarter).
#[test]
fn kv_at_peak_is_all_or_nothing_and_scales_exactly() {
    assert!(kv_at_peak(None, Some(500)).is_none());
    assert!(kv_at_peak(Some((32, 32, 8, 4096, false)), None).is_none());
    let kv = kv_at_peak(Some((32, 32, 8, 4096, true)), Some(1000)).unwrap();
    assert!(kv.conservative);
    assert_eq!(kv.peak_tokens, 1000);
    assert_eq!(kv.f16_bytes / kv.q8_0_bytes, 2);
    assert_eq!(kv.f16_bytes / kv.q4_0_bytes, 4);
}

/// Provenance strings name whichever measurement exists — resident (Ollama) beats
/// on-disk (llama.cpp); neither → an explicit not-measurable, never a silent 0.
#[test]
fn memory_facts_name_their_provenance() {
    let ollama = BatchColumn { weights_vram_bytes: Some(8), weights_total_bytes: Some(9), ..Default::default() };
    let f = memory_facts(Some(&ollama), None);
    assert_eq!(f.model_bytes, Some(8));
    assert!(f.model_bytes_provenance.contains("size_vram"));
    let llama = BatchColumn { weights_total_bytes: Some(9), ..Default::default() };
    let f = memory_facts(Some(&llama), None);
    assert_eq!(f.model_bytes, Some(9));
    assert!(f.model_bytes_provenance.contains("GGUF"));
    let none = memory_facts(None, None);
    assert_eq!(none.model_bytes, None);
    assert!(none.model_bytes_provenance.contains("not measurable"));
}

/// Per-task KV: with dims present, each row carries the f16 figure at ITS OWN peak
/// (not the collection peak); with dims unmeasurable it stays None — never a guess.
#[test]
fn per_task_kv_uses_each_tasks_own_peak_and_gates_on_dims() {
    let mut a = step(0, 0);
    a.context_used = Some(1000);
    let mut b = step(0, 0);
    b.context_used = Some(500);
    let mut cells = std::collections::BTreeMap::new();
    cells.insert(("t-big".to_string(), false), vec![a]);
    cells.insert(("t-small".to_string(), false), vec![b]);
    let dims = Some((32, 32, 8, 4096, false));
    let c = assemble("m", &cells, &std::collections::BTreeMap::new(), None, dims);
    let big = c.tasks.iter().find(|t| t.task_id == "t-big").unwrap();
    let small = c.tasks.iter().find(|t| t.task_id == "t-small").unwrap();
    let (kb, ks) = (big.kv_f16_bytes_at_peak.unwrap(), small.kv_f16_bytes_at_peak.unwrap());
    // Twice the occupancy → twice the KV (the formula is linear in tokens).
    assert_eq!(kb, ks * 2);
    // Footer figure sizes at the COLLECTION peak = the big task's.
    assert_eq!(c.memory.kv_at_peak.as_ref().unwrap().f16_bytes, kb);
    // No dims → no per-task figure, never a guess.
    let c2 = assemble("m", &cells, &std::collections::BTreeMap::new(), None, None);
    assert!(c2.tasks.iter().all(|t| t.kv_f16_bytes_at_peak.is_none()));
}
