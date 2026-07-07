#![deny(clippy::unwrap_used)]
use crate::errors::AppError;
use crate::inference::compare::compare_runner::RowSpec;
use crate::inference::compare::compare_sink::CompareSink;
use crate::inference::generate::generate_stats::GenerateStats;
use crate::metrics::timing::RunTiming;
use crate::sync::MutexExt;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

pub(crate) fn finalize_row(
    sink: &dyn CompareSink,
    row: &RowSpec,
    timing: &Arc<Mutex<RunTiming>>,
    row_token: &CancellationToken,
    result: Result<GenerateStats, AppError>,
) {
    let id = row.model_id.to_string();
    match result {
        Ok(_) if row_token.is_cancelled() => {
            sink.cancelled(&id, &row.model, timing.lock_recover().token_count);
        }
        Ok(stats) => {
            let t = timing.lock_recover();
            sink.done(&id, &row.model, t.ttft_ms(), t.tokens_per_sec(), t.token_count, t.timeline(), &stats);
        }
        Err(err) => {
            let (kind, message) = app_error_split(&err);
            sink.error(&id, &row.model, &kind, &message);
        }
    }
}

fn app_error_split(e: &AppError) -> (String, String) {
    let (kind, m) = match e {
        AppError::Validation(m) => ("validation", m),
        AppError::Truncated(m) => ("truncated", m),
        AppError::InvalidTaskSchema(m) => ("invalid_task_schema", m),
        AppError::NotFound(m) => ("not_found", m),
        AppError::Inference(m) => ("inference", m),
        AppError::Io(m) => ("io", m),
        AppError::Timeout(m) => ("timeout", m),
        AppError::AuthRequired(m) => ("auth_required", m),
        AppError::Internal(m) => ("internal", m),
        AppError::Cancelled(m) => ("cancelled", m),
    };
    (kind.into(), m.clone())
}
