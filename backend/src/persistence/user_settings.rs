use crate::errors::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::path::Path;

fn is_false(b: &bool) -> bool { !*b }

/// App-wide user preferences persisted to user_settings.yaml in the app
/// config dir. Grows across phases: theme (2.7), onboarding flag (2.6),
/// last update check (2.9).
#[derive(Serialize, Deserialize, Default, PartialEq, Debug, Clone)]
pub struct UserSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub first_run_complete: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_update_check_at: Option<String>,
    /// Override for the shared GGUF weights folder (default `~/.quantamind/gguf`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models_folder: Option<String>,
    /// Folder holding a user-installed `whisper-server` (STT engine), set via the
    /// Speech-to-Text setup card's folder picker. Persisted so a custom install
    /// is found on every launch without re-picking. Consulted first by
    /// `whisper_dir`, ahead of PATH/Homebrew discovery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stt_engine_dir: Option<String>,
    /// Base URL of a remote vLLM OpenAI-compatible server (e.g.
    /// `http://34.10.20.30:8000`). vLLM/SGLang run on a remote GPU, so — unlike the
    /// localhost sidecars — their endpoint is user-configured. Empty/unset ⇒ the
    /// vLLM backend reports "not configured".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vllm_url: Option<String>,
    /// Bearer token for the vLLM server, if it was launched with `--api-key`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vllm_api_key: Option<String>,
    /// Base URL of a remote SGLang OpenAI-compatible server (e.g.
    /// `http://34.10.20.30:30000`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sglang_url: Option<String>,
    /// Bearer token for the SGLang server, if it was launched with `--api-key`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sglang_api_key: Option<String>,
}

pub fn load(path: &Path) -> AppResult<UserSettings> {
    if !path.exists() {
        return Ok(UserSettings::default());
    }
    let content = std::fs::read_to_string(path).map_err(|e| AppError::Io(e.to_string()))?;
    if content.trim().is_empty() {
        return Ok(UserSettings::default());
    }
    serde_yaml::from_str(&content).map_err(|e| AppError::Internal(e.to_string()))
}

pub fn save(path: &Path, s: &UserSettings) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }
    // Secrets never touch disk (rule 7a). The API-key fields are kept on the struct for the
    // IPC contract, but the keychain (`SecureSecrets`) is their only durable store, so this
    // boundary strips them before serializing — regardless of caller. See docs/security.md.
    let mut on_disk = s.clone();
    on_disk.vllm_api_key = None;
    on_disk.sglang_api_key = None;
    let yaml = serde_yaml::to_string(&on_disk).map_err(|e| AppError::Internal(e.to_string()))?;
    std::fs::write(path, yaml).map_err(|e| AppError::Io(e.to_string()))
}

#[cfg(test)]
#[path = "user_settings_tests.rs"]
mod tests;
