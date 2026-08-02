// `llama_discover` (a pure GGUF-folder scan), `llama_runtime` (reachability +
// context-ceiling math) and `llama_server_types` carry no Tauri dependency, so
// they stay in the headless `qm` build. The modules that spawn/own the sidecar
// through an `AppHandle` are GUI-only.
pub mod llama_discover;
pub mod llama_runtime;
pub mod llama_server_types;
#[cfg(feature = "gui")]
pub mod llama_models;
#[cfg(feature = "gui")]
pub mod llama_start;
#[cfg(feature = "gui")]
pub mod llama_templates;
