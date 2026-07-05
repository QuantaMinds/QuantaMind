use crate::commands::storage::storage_disk::{gguf_dir_resolved, mlx_dir_resolved};
use crate::errors::{AppError, AppResult};
use crate::inference::backend::remote_config;
use crate::persistence::user_settings::{load, save, UserSettings};
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
        let loaded_settings = load(&settings_path(app)?)?;
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

    /// The resolved MLX weights folder (env → `~/.quantamind/mlx`). Independent
    /// of the GGUF folder so safetensors snapshots don't co-mingle with GGUFs.
    pub fn mlx_weights_dir(&self) -> PathBuf {
        mlx_dir_resolved(None)
    }

    /// The user-set custom folder for the whisper-server STT engine, if any.
    /// `whisper_dir` consults this first so a manually-located install persists
    /// across launches.
    pub fn stt_engine_dir(&self, app: &tauri::AppHandle) -> AppResult<Option<String>> {
        self.ensure_loaded(app)?;
        Ok(self.inner.lock_recover().stt_engine_dir.clone())
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
    push_remote_endpoints(&settings);
    *state.inner.lock_recover() = settings.clone();
    save(&settings_path(&app)?, &settings)
}

/// The absolute shared GGUF weights folder, for display in the UI.
#[tauri::command]
pub fn resolve_models_folder(
    app: tauri::AppHandle,
    state: tauri::State<'_, UserSettingsState>,
) -> Result<String, AppError> {
    Ok(state.weights_dir(&app)?.to_string_lossy().into_owned())
}
