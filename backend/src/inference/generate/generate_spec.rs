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
    /// `Some(false)` actively DISABLES thinking on Ollama — required for thinking-BY-DEFAULT
    /// models (qwen3*), which otherwise reason anyway and burn a non-thinking turn's budget in a
    /// hidden block; `None` = backend default. Per backend: Ollama → `think` request field
    /// (`false` is accepted by all versions; only `true` is capability-checked); MLX/vLLM/SGLang →
    /// `chat_template_kwargs.enable_thinking`, where `Some(false)` and `None` are identical
    /// (both send `false`); llama.cpp emits reasoning in `reasoning_content` regardless
    /// (captured in the wire layer, no request flag needed).
    pub think: Option<bool>,
}
