use serde::{Deserialize, Serialize};

/// The per-run inference sampling params. Lives in the domain (not `persistence`)
/// so the eval engine can stamp them on a `BatchReport` without importing the
/// persistence layer (see docs/architecture.md#layering); `persistence::prompts::schema`
/// re-exports it for the storage/IPC call sites.
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone, Default)]
pub struct InferenceParams {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub repeat_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub num_ctx: Option<u32>,
}
