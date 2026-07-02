use crate::inference::generate::generate_options::GenerateOptions;

/// The inputs to one generation, grouped so every backend shares a
/// single shape. Fields are owned so a spec can move into a spawned task.
#[derive(Clone, Debug, Default)]
pub struct GenerateSpec {
    pub model: String,
    pub prompt: String,
    pub system: Option<String>,
    pub options: Option<GenerateOptions>,
    pub keep_alive: Option<i32>,
    /// Ask Ollama to emit its reasoning in the separate `thinking` channel (`/api/generate`
    /// `think:true`). Set for reasoning models so the harness can CAPTURE the scratchpad
    /// (else it lands off the `response` channel, invisible, while still burning `num_predict`).
    /// `None`/`Some(false)` = default. Per backend: Ollama → `think` request field; MLX →
    /// `chat_template_kwargs.enable_thinking` (a `has_thinking` model reasons by default, so
    /// `Some(true)` is needed to CAPTURE and the absence suppresses it); llama.cpp emits reasoning
    /// in `reasoning_content` regardless (captured in the wire layer, no request flag needed).
    pub think: Option<bool>,
}
