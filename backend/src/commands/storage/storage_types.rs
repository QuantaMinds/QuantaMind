use crate::inference::backend::backend_kind::BackendKind;
use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct InstalledModelInfo {
    pub name: String,
    pub size_bytes: u64,
    pub modified_at: String,
    pub family: String,
    pub parameter_size: String,
    pub quantization: String,
    pub backend: BackendKind,
    /// Content hash identifying the underlying model blob, when a backend
    /// exposes one. Empty for llama.cpp GGUF entries, where each file is
    /// already unique; the picker collapses duplicates on it when present.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub digest: String,
    /// Friendly label for the picker when `name` is not presentable. `None`
    /// for backends whose `name` is already friendly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Absolute GGUF path — set for llama.cpp models (used to launch the
    /// sidecar on the right file); `None` for remote backends.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct DiskUsage {
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub models_bytes: u64,
}
