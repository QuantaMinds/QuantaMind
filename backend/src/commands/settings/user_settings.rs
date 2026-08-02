use crate::commands::storage::storage_disk::gguf_dir_resolved;
use crate::errors::{AppError, AppResult};
use crate::inference::backend::remote_config;
use crate::inference::backend::remote_guard::credential_allowed;
use crate::persistence::user_settings::{load, save, UserSettings};
use crate::secrets::{self, Persisted};
use crate::sync::MutexExt;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::Manager;

pub const USER_SETTINGS_FILE: &str = "user_settings.yaml";

/// Mirror the remote vLLM/SGLang endpoint settings into the `inference/`
/// process-global so the dispatch path (which can't read Tauri state) resolves
/// them. Called on first load and on every save.
fn push_remote_endpoints(s: &UserSettings) {
    remote_config::set_vllm(s.vllm_url.clone(), s.vllm_api_key.clone());
    remote_config::set_sglang(s.sglang_url.clone(), s.sglang_api_key.clone());
}

/// Store one API key in the keychain, or clear it when blanked. Returns whether it landed
/// durably (`true` when stored to the keychain, or when there was nothing to store).
fn store_or_clear(key: &str, val: Option<&str>) -> bool {
    match val.map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) => secrets::store(key, v) == Persisted::Keychain,
        None => {
            secrets::clear(key);
            true
        }
    }
}

/// Route the two cloud API keys to the keychain (rule 7a). Best-effort: keys always reach
/// the session store even if the keychain is denied. Used on every save.
fn persist_api_keys(s: &UserSettings) {
    store_or_clear(secrets::VLLM_API_KEY, s.vllm_api_key.as_deref());
    store_or_clear(secrets::SGLANG_API_KEY, s.sglang_api_key.as_deref());
}

/// Guardrail (rule 7d): refuse to accept an API key bound to a cleartext `http://` remote
/// endpoint, so the key can never be sent in the clear. Rejected at save so the bad state is
/// never stored. Loopback http is fine (no network to sniff); https is fine.
fn reject_cleartext_credentials(s: &UserSettings) -> AppResult<()> {
    check_credential_transport("vLLM", s.vllm_url.as_deref(), s.vllm_api_key.as_deref())?;
    check_credential_transport("SGLang", s.sglang_url.as_deref(), s.sglang_api_key.as_deref())
}

fn check_credential_transport(label: &str, url: Option<&str>, key: Option<&str>) -> AppResult<()> {
    let key = key.map(str::trim).filter(|k| !k.is_empty());
    let url = url.map(str::trim).filter(|u| !u.is_empty());
    if let (Some(url), Some(_)) = (url, key) {
        if !credential_allowed(url) {
            return Err(AppError::Validation(format!(
                "{label}: refusing to store the API key — {url} is not HTTPS, so the key would \
                 be sent in cleartext. Use an https:// URL, or clear the key if the server has \
                 no auth."
            )));
        }
    }
    Ok(())
}

/// On load, move any legacy plaintext keys out of the YAML into the keychain, then hydrate
/// the in-memory copy from the keychain. Mutates `s` to hold the live keys. Returns `true`
/// when a legacy plaintext key was found AND durably re-homed, so the caller must rewrite
/// the YAML to strip it. If the keychain was unavailable we leave the file untouched this
/// launch (never destroy the user's only copy) and retry next launch.
fn migrate_and_hydrate_keys(s: &mut UserSettings) -> bool {
    let had_plaintext = s.vllm_api_key.is_some() || s.sglang_api_key.is_some();
    let mut durable = true;
    if let Some(k) = s.vllm_api_key.as_deref() {
        durable &= store_or_clear(secrets::VLLM_API_KEY, Some(k));
    }
    if let Some(k) = s.sglang_api_key.as_deref() {
        durable &= store_or_clear(secrets::SGLANG_API_KEY, Some(k));
    }
    s.vllm_api_key = secrets::get(secrets::VLLM_API_KEY);
    s.sglang_api_key = secrets::get(secrets::SGLANG_API_KEY);
    had_plaintext && durable
}

#[derive(Default)]
pub struct UserSettingsState {
    inner: Mutex<UserSettings>,
    loaded: Mutex<bool>,
}

fn settings_path(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    let dir = app.path().app_config_dir().map_err(|e| AppError::Io(e.to_string()))?;
    Ok(dir.join(USER_SETTINGS_FILE))
}

impl UserSettingsState {
    fn ensure_loaded(&self, app: &tauri::AppHandle) -> AppResult<()> {
        let mut loaded = self.loaded.lock_recover();
        if *loaded {
            return Ok(());
        }
        let path = settings_path(app)?;
        let mut loaded_settings = load(&path)?;
        // Migrate any legacy plaintext keys into the keychain + hydrate from it. Only rewrite
        // the file to strip plaintext once the key is durably re-homed (never lose the copy).
        if migrate_and_hydrate_keys(&mut loaded_settings) {
            save(&path, &loaded_settings)?;
        }
        push_remote_endpoints(&loaded_settings);
        *self.inner.lock_recover() = loaded_settings;
        *loaded = true;
        Ok(())
    }

    /// The resolved shared GGUF weights folder (user setting → env → default).
    pub fn weights_dir(&self, app: &tauri::AppHandle) -> AppResult<PathBuf> {
        self.ensure_loaded(app)?;
        let folder = self.inner.lock_recover().models_folder.clone();
        Ok(gguf_dir_resolved(folder.as_deref()))
    }
}

#[tauri::command]
pub fn get_user_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, UserSettingsState>,
) -> Result<UserSettings, AppError> {
    state.ensure_loaded(&app)?;
    Ok(state.inner.lock_recover().clone())
}

#[tauri::command]
pub fn set_user_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, UserSettingsState>,
    settings: UserSettings,
) -> Result<(), AppError> {
    state.ensure_loaded(&app)?;
    reject_cleartext_credentials(&settings)?;
    persist_api_keys(&settings);
    push_remote_endpoints(&settings);
    *state.inner.lock_recover() = settings.clone();
    // `save` strips the API-key fields; the keychain (above) is their only durable store.
    let result = save(&settings_path(&app)?, &settings);
    if result.is_ok() {
        crate::audit::record(crate::audit::AuditEvent::SettingsChanged); // audit seam (no-op in OSS)
    }
    result
}

/// The absolute shared GGUF weights folder, for display in the UI.
#[tauri::command]
pub fn resolve_models_folder(
    app: tauri::AppHandle,
    state: tauri::State<'_, UserSettingsState>,
) -> Result<String, AppError> {
    Ok(state.weights_dir(&app)?.to_string_lossy().into_owned())
}

#[cfg(test)]
#[path = "user_settings_tests.rs"]
mod tests;
