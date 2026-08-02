use super::*;
use crate::errors::AppError;
use crate::inference::eval::toolcall::eval::ToolCallReport;

fn target(model: &str) -> ModelTarget {
    ModelTarget { model: model.into(), backend: BackendKind::LlamaCpp, is_thinking: false }
}

fn report(composite: Option<f64>) -> ToolCallReport {
    ToolCallReport {
        n: 3,
        parse_rate: Some(1.0),
        tool_selection_acc: Some(0.5),
        arg_acc: Some(0.5),
        abstain_acc: None,
        composite,
        prompt_tokens: None,
        per_task: Vec::new(),
    }
}

#[test]
fn build_matrix_maps_targets_and_captures_errors() {
    let results = vec![
        (target("a"), Ok(report(Some(0.8)))),
        (target("b"), Err(AppError::Inference("server down".into()))),
    ];
    let m = build_matrix("mine", results);
    assert_eq!(m.collection_id, "mine");
    assert_eq!(m.columns.len(), 2);
    assert!(m.columns[0].report.is_some() && m.columns[0].error.is_none());
    assert!(m.columns[1].report.is_none());
    assert!(m.columns[1].error.as_deref().unwrap().contains("server down"));
}

#[test]
fn avg_score_ignores_failed_and_none() {
    let results = vec![
        (target("a"), Ok(report(Some(0.8)))),
        (target("b"), Ok(report(Some(0.4)))),
        (target("c"), Ok(report(None))),              // composite missing → ignored
        (target("d"), Err(AppError::Inference("x".into()))), // failed → ignored
    ];
    let m = build_matrix("mine", results);
    assert!((m.avg_score.unwrap() - 0.6).abs() < 1e-9); // mean of 0.8 and 0.4
}

#[test]
fn avg_score_none_when_no_successes() {
    let m = build_matrix("mine", vec![(target("a"), Err(AppError::Inference("x".into())))]);
    assert_eq!(m.avg_score, None);
}

#[test]
fn summaries_one_per_successful_column() {
    let results = vec![
        (target("a"), Ok(report(Some(0.8)))),
        (target("b"), Err(AppError::Inference("down".into()))),
    ];
    let m = build_matrix("mine", results);
    let s = summaries(&m, "2026-06-03T00:00:00Z");
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].model, "a");
    assert_eq!(s[0].ts, "2026-06-03T00:00:00Z");
    assert_eq!(s[0].composite, Some(0.8));
}
