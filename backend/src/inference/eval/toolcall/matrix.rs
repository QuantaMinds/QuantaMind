use crate::errors::AppResult;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::eval::run_summary::RunSummary;
use crate::inference::eval::toolcall::eval::ToolCallReport;
use serde::{Deserialize, Serialize};

/// One model+backend to run a collection against (sent by the frontend; also
/// persisted in the resumable job-queue header, hence `Serialize`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ModelTarget {
    pub model: String,
    pub backend: BackendKind,
    /// Reasoning model (the sidebar "thinking" checkbox). Drives the raised per-turn token
    /// budget + `<think>` stripping in the agentic runner. `#[serde(default)]` so a job log
    /// written before this field (or a non-thinking target) deserializes as `false`.
    #[serde(default)]
    pub is_thinking: bool,
}

/// One model's result for the whole collection: either a full report or the
/// error that model hit (a down backend must not fail the rest of the matrix).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct MatrixColumn {
    pub model: String,
    pub backend: BackendKind,
    pub report: Option<ToolCallReport>,
    pub error: Option<String>,
}

/// The full matrix: one column per target plus the mean composite across the
/// columns that succeeded.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct MatrixReport {
    pub collection_id: String,
    pub columns: Vec<MatrixColumn>,
    pub avg_score: Option<f64>,
}

/// Fold per-target run outcomes into a matrix report. Pure (no async / no I/O) so
/// the mapping + averaging is unit-testable without a model server.
pub fn build_matrix(
    collection_id: &str,
    results: Vec<(ModelTarget, AppResult<ToolCallReport>)>,
) -> MatrixReport {
    let columns: Vec<MatrixColumn> = results
        .into_iter()
        .map(|(target, res)| match res {
            Ok(report) => MatrixColumn { model: target.model, backend: target.backend, report: Some(report), error: None },
            Err(e) => MatrixColumn { model: target.model, backend: target.backend, report: None, error: Some(e.to_string()) },
        })
        .collect();

    let scored: Vec<f64> = columns
        .iter()
        .filter_map(|c| c.report.as_ref().and_then(|r| r.composite))
        .collect();
    let avg_score = if scored.is_empty() { None } else { Some(scored.iter().sum::<f64>() / scored.len() as f64) };

    MatrixReport { collection_id: collection_id.to_string(), columns, avg_score }
}

/// One history `RunSummary` per successful column, stamped with `ts`. Failed
/// columns are not recorded (only real measurements enter the regression log).
pub fn summaries(report: &MatrixReport, ts: &str) -> Vec<RunSummary> {
    report
        .columns
        .iter()
        .filter_map(|c| {
            let r = c.report.as_ref()?;
            Some(RunSummary {
                ts: ts.to_string(),
                model: c.model.clone(),
                backend: c.backend,
                parse_rate: r.parse_rate,
                tool_selection_acc: r.tool_selection_acc,
                arg_acc: r.arg_acc,
                abstain_acc: r.abstain_acc,
                composite: r.composite,
                n: r.n,
                pass_k: None,
                agentic_avg_steps: None,
                effort: None,
                is_thinking: false, // single-turn matrix: no agentic effort, thinking N/A
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "matrix_tests.rs"]
mod tests;
