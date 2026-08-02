// Tauri command handlers, grouped by concern (see docs/architecture.md#folder-taxonomy).
//
// Feature seam: modules the headless `qm` CLI reaches (eval, mcp, models,
// prompt, remote, system) compile in every build — their `#[tauri::command]`
// wrappers are gated item-by-item inside each file. Everything else is
// GUI-only and gated out of the `--no-default-features` CLI build wholesale.
#[cfg(feature = "gui")]
pub mod app_lifecycle;
#[cfg(feature = "gui")]
pub mod compare;
#[cfg(feature = "gui")]
pub mod emit;
pub mod eval;
#[cfg(feature = "gui")]
pub mod gguf;
#[cfg(feature = "gui")]
pub mod hf;
#[cfg(feature = "gui")]
pub mod llama;
pub mod mcp;
#[cfg(feature = "gui")]
pub mod mlx;
pub mod models;
#[cfg(feature = "gui")]
pub mod ollama;
pub mod prompt;
#[cfg(feature = "gui")]
pub mod prompt_templates;
#[cfg(feature = "gui")]
pub mod publish;
pub mod remote;
#[cfg(feature = "gui")]
pub mod settings;
#[cfg(feature = "gui")]
pub mod storage;
pub mod system;
#[cfg(feature = "gui")]
pub mod workspace;
