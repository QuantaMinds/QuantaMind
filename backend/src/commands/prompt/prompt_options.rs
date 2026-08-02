use crate::errors::{AppError, AppResult};
use crate::inference::generate::generate_options::GenerateOptions;
use crate::persistence::prompts::schema::InferenceParams;

fn in_range(name: &str, v: Option<f32>, lo: f32, hi: f32) -> AppResult<()> {
    if let Some(x) = v {
        if !x.is_finite() || x < lo || x > hi {
            return Err(AppError::Validation(format!("{name} must be {lo}–{hi}, got {x}")));
        }
    }
    Ok(())
}

pub fn validate_params(p: &InferenceParams) -> AppResult<()> {
    in_range("temperature", p.temperature, 0.0, 2.0)?;
    in_range("top_p", p.top_p, 0.0, 1.0)?;
    in_range("repeat_penalty", p.repeat_penalty, 0.0, 2.0)?;
    if let Some(n) = p.num_ctx {
        if n < 1 {
            return Err(AppError::Validation("num_ctx must be at least 1".into()));
        }
    }
    Ok(())
}

/// Map persisted per-prompt params onto the server's option block.
/// `max_tokens` becomes the server's `num_predict`; `num_ctx` is the context window.
pub fn to_generate_options(p: &InferenceParams) -> GenerateOptions {
    GenerateOptions {
        temperature: p.temperature,
        top_p: p.top_p,
        top_k: p.top_k,
        num_predict: p.max_tokens,
        repeat_penalty: p.repeat_penalty,
        seed: p.seed,
        num_ctx: p.num_ctx,
        // The Workspace prompt path uses the model's own Modelfile stops; per-model stop
        // injection is the eval harness's job (see `BackendTurn::run`).
        stop: None,
    }
}

#[cfg(test)]
#[path = "prompt_options_tests.rs"]
mod tests;
