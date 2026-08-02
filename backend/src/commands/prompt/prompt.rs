#![deny(clippy::unwrap_used)]

use crate::commands::settings::model_settings::ModelSettingsState;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::backend::endpoint;
use crate::inference::token_handler::make_token_handler;
use crate::commands::prompt::prompt_options::{to_generate_options, validate_params};
use crate::commands::prompt::prompt_payloads::{done_payload, CancelledPayload, TokenPayload};
pub use crate::commands::prompt::prompt_run::run_prompt_inner;
use crate::errors::AppError;
use crate::metrics::timing::RunTiming;
use crate::persistence::prompts::schema::InferenceParams;
use crate::sync::MutexExt;
use std::sync::{Arc, Mutex};
use tauri::Emitter;
use tokio_util::sync::CancellationToken;

pub const EVENT_TOKEN: &str = "prompt-token";
pub const EVENT_DONE: &str = "prompt-done";
pub const EVENT_CANCELLED: &str = "prompt-cancelled";

#[derive(Default)]
pub struct RunState {
    current: Mutex<Option<CancellationToken>>,
}

#[tauri::command]
pub async fn run_prompt(
    app: tauri::AppHandle,
    state: tauri::State<'_, RunState>,
    settings: tauri::State<'_, ModelSettingsState>,
    model: String,
    prompt: String,
    system: Option<String>,
    params: Option<InferenceParams>,
    backend: Option<BackendKind>,
    keep_alive: Option<i32>,
) -> Result<(), AppError> {
    let backend = backend.unwrap_or_default();
    settings.ensure_loaded(&app)?;
    if let Some(p) = &params {
        validate_params(p)?;
    }
    let mut options = params.as_ref().map(to_generate_options).unwrap_or_default();
    // Fall back to the per-model temperature when the prompt didn't set one.
    if options.temperature.is_none() {
        options.temperature = Some(settings.temperature_for(&model));
    }
    // Resolve the endpoint up front (llama.cpp locally, or the user-configured
    // vLLM/SGLang URL) so an unconfigured remote errors with a clear message BEFORE we
    // spin up the run token/timing.
    let resolved = endpoint::resolve(backend)?;

    let token = CancellationToken::new();
    {
        let mut guard = state.current.lock_recover();
        if let Some(prev) = guard.take() { prev.cancel(); }
        *guard = Some(token.clone());
    }

    let timing = Arc::new(Mutex::new(RunTiming::start()));
    let emit_app = app.clone();
    let handler = make_token_handler(
        move |t| emit_app
            .emit(EVENT_TOKEN, TokenPayload { text: t.to_string() })
            .map_err(|_| ()),
        token.clone(),
        timing.clone(),
    );
    let system_trim = system.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let result = run_prompt_inner(
        backend, &resolved.url, &model, &prompt, system_trim,
        Some(options), keep_alive, token.clone(), handler,
    ).await;

    *state.current.lock_recover() = None;

    if let Ok(stats) = &result {
        if token.is_cancelled() {
            let count = timing.lock_recover().token_count;
            app.emit(EVENT_CANCELLED, CancelledPayload { token_count: count })
                .map_err(|e| AppError::Internal(e.to_string()))?;
        } else {
            let payload = done_payload(&timing, stats);
            app.emit(EVENT_DONE, &payload)
                .map_err(|e| AppError::Internal(e.to_string()))?;
        }
    }
    result.map(|_| ())
}

#[tauri::command]
pub fn stop_prompt(state: tauri::State<'_, RunState>) -> Result<(), AppError> {
    if let Some(token) = state.current.lock_recover().take() {
        token.cancel();
    }
    Ok(())
}
