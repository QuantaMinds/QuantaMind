use crate::commands::eval::evals_load::load_all;
use crate::commands::prompt::prompt_run::run_prompt_inner;
use crate::errors::AppError;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::backend::endpoint;
use crate::inference::eval::eval_score::score;
use crate::inference::eval::eval_task::EvalTask;
use crate::inference::generate::generate_options::GenerateOptions;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

const MAX_TOKENS: u32 = 256;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct EvalRunResult {
    pub task_id: String,
    pub category: String,
    pub passed: bool,
    pub detail: String,
    pub output: String,
    pub token_count: u32,
}

/// Run one eval task against a backend, accumulate the full output (no events —
/// evals don't stream to the UI), and score it. Temperature 0 for determinism.
pub async fn run_and_score(
    backend: BackendKind,
    endpoint: &str,
    model: &str,
    task: &EvalTask,
) -> Result<EvalRunResult, AppError> {
    let options = GenerateOptions {
        temperature: Some(0.0),
        num_predict: Some(MAX_TOKENS),
        ..Default::default()
    };
    let mut output = String::new();
    let mut count = 0u32;
    run_prompt_inner(
        backend,
        endpoint,
        model,
        &task.prompt,
        None,
        Some(options),
        None,
        CancellationToken::new(),
        |t| {
            output.push_str(t);
            count += 1;
        },
    )
    .await?;
    let s = score(task, &output);
    Ok(EvalRunResult {
        task_id: task.id.clone(),
        category: task.category.clone(),
        passed: s.passed,
        detail: s.detail,
        output,
        token_count: count,
    })
}

fn endpoint_for(backend: BackendKind) -> String {
    endpoint::base_url(backend)
}

#[tauri::command]
pub async fn run_eval_task(
    app: tauri::AppHandle,
    task_id: String,
    model: String,
    backend: Option<BackendKind>,
) -> Result<EvalRunResult, AppError> {
    let backend = backend.unwrap_or_default();
    let task = load_all(&app)?
        .into_iter()
        .find(|t| t.id == task_id)
        .ok_or_else(|| AppError::Validation(format!("unknown eval: {task_id}")))?;
    run_and_score(backend, &endpoint_for(backend), &model, &task).await
}
