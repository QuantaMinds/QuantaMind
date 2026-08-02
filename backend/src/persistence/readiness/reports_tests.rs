use super::*;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::eval::agentic::scoring::report::{FailureTracker, TopError};
use crate::inference::eval::batch::{AggAgentic, BatchColumn};
use tempfile::tempdir;

fn report(collection_id: &str, passes: u32) -> BatchReport {
    BatchReport {
        unreadable_columns: 0,
        collection_id: collection_id.into(),
        num_ctx: Some(8192),
        collection_hash: None,
        think_preset: None,
        params: None,
        columns: vec![BatchColumn {
            model: "qwen".into(),
            backend: BackendKind::LlamaCpp,
            toolcall: None,
            agentic: Some(AggAgentic {
                tasks_passed: passes,
                tasks_total: 5,
                passes,
                total_runs: 5,
                avg_steps: Some(2.4),
                avg_output_tokens_success: Some(110.0),
                schema_resilience: None,
                top_error: TopError::None,
                failures: FailureTracker::default(),
                by_tier: vec![],
                tasks_errored: 0,
                native_error_class: Default::default(),
                boundary: None,
                tokens_per_completed: None,
            diagnostic: None,
            native_structured_calls: None,
            native_salvaged_calls: None,
            }),
            agentic_native_fc: None,
            error: None,
            is_thinking: false,
            cpu_offloaded: false,
            ctx_ceiling: None,
            ..Default::default()
        }],
    }
}

#[test]
fn save_then_load_round_trips_the_report() {
    let dir = tempdir().unwrap();
    let r = report("finance", 4);
    save(dir.path(), &r).unwrap();
    let loaded = load(dir.path(), "finance").unwrap();
    assert_eq!(loaded, Some(r));
    assert_eq!(loaded.unwrap().num_ctx, Some(8192)); // the run's context length round-trips
}

#[test]
fn pre_7_4_report_without_num_ctx_still_loads() {
    // Back-compat: a report persisted before Phase 7.4 omits `num_ctx`;
    // #[serde(default)] must fill it with None rather than fail the load.
    let dir = tempdir().unwrap();
    let json = r#"{"collection_id":"old","columns":[]}"#;
    std::fs::create_dir_all(dir.path()).unwrap();
    std::fs::write(report_path(dir.path(), "old"), json).unwrap();
    let loaded = load(dir.path(), "old").unwrap().unwrap();
    assert_eq!(loaded.num_ctx, None);
}

#[test]
fn pre_7_2_column_without_native_fc_loads_as_none() {
    // Back-compat: a column persisted before Phase 7.2 has no `agentic_native_fc`
    // key; #[serde(default)] must fill it with None rather than fail the load.
    let dir = tempdir().unwrap();
    let json = r#"{"collection_id":"old","num_ctx":4096,"columns":[
        {"model":"m","backend":"llama_cpp","toolcall":null,"agentic":null,"error":null}
    ]}"#;
    std::fs::create_dir_all(dir.path()).unwrap();
    std::fs::write(report_path(dir.path(), "old"), json).unwrap();
    let loaded = load(dir.path(), "old").unwrap().unwrap();
    assert!(loaded.columns[0].agentic_native_fc.is_none());
}

#[test]
fn load_missing_is_none_not_error() {
    let dir = tempdir().unwrap();
    assert_eq!(load(dir.path(), "never-run").unwrap(), None);
}

#[test]
fn save_is_last_write_wins() {
    let dir = tempdir().unwrap();
    save(dir.path(), &report("c", 1)).unwrap();
    save(dir.path(), &report("c", 5)).unwrap();
    let loaded = load(dir.path(), "c").unwrap().unwrap();
    assert_eq!(loaded.columns[0].agentic.as_ref().unwrap().passes, 5);
}

#[test]
fn long_collection_ids_do_not_collide() {
    let dir = tempdir().unwrap();
    let base = "company-evals-qwen3-coder-agentic-v1-test-suite";
    save(dir.path(), &report(&format!("{base}-AAAA"), 1)).unwrap();
    save(dir.path(), &report(&format!("{base}-BBBB"), 5)).unwrap();
    assert_eq!(load(dir.path(), &format!("{base}-AAAA")).unwrap().unwrap().columns[0].agentic.as_ref().unwrap().passes, 1);
    assert_eq!(load(dir.path(), &format!("{base}-BBBB")).unwrap().unwrap().columns[0].agentic.as_ref().unwrap().passes, 5);
}

/// Same migration guard as the history/trace stores: a report whose columns name a
/// now-removed backend must not blank the whole Agent Report page. The readable
/// columns load and the dropped ones are COUNTED, so the verdict table can say
/// what it isn't showing rather than passing a short list off as the full run.
#[test]
fn a_column_naming_an_unsupported_backend_is_skipped_not_fatal() {
    let dir = tempdir().unwrap();
    let json = r#"{
        "collection_id":"c",
        "columns":[
            {"model":"legacy","backend":"retired_engine","toolcall":null,"agentic":null,"error":null},
            {"model":"current","backend":"llama_cpp","toolcall":null,"agentic":null,"error":null}
        ]
    }"#;
    std::fs::create_dir_all(dir.path()).unwrap();
    std::fs::write(dir.path().join(safe_filename("c")).with_extension("json"), json).unwrap();

    let r = load(dir.path(), "c").unwrap().expect("report loads");
    assert_eq!(r.columns.len(), 1, "the readable column still loads");
    assert_eq!(r.columns[0].model, "current");
    assert_eq!(r.unreadable_columns, 1, "the legacy column is counted, not silently dropped");
}

/// A report with nothing to skip reports zero — the count must not be a
/// permanently-on caveat that trains the user to ignore it.
#[test]
fn a_fully_readable_report_counts_zero_unreadable_columns() {
    let dir = tempdir().unwrap();
    save(dir.path(), &report("c", 5)).unwrap();
    let r = load(dir.path(), "c").unwrap().expect("report loads");
    assert_eq!(r.unreadable_columns, 0);
}
