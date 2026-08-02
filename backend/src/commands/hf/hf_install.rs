use crate::commands::emit::log_emit;
use crate::commands::gguf::gguf_cmd::EVENT_MODELS_CHANGED;
use crate::commands::hf::hf_phase::{HfPhase, EVENT_HF_PROGRESS};
use crate::commands::settings::user_settings::UserSettingsState;
use crate::commands::storage::storage_disk::gguf_dest;
use std::path::PathBuf;
use crate::errors::{AppError, AppResult};
use crate::inference::hf::hf_download::{download_gguf, DownloadProgress};
use crate::inference::hf::hf_resume::partial_path;
use crate::inference::pull::pull_name::validate_name;
use crate::sync::MutexExt;
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use tauri::AppHandle;
use tokio_util::sync::CancellationToken;

const HF_ENDPOINT: &str = "https://huggingface.co";

#[derive(Default)]
pub struct HfInstallState {
    current: Mutex<Option<CancellationToken>>,
}

impl HfInstallState {
    /// The shared single-in-flight token slot, so one cancel channel and one
    /// one-at-a-time guard cover every install path.
    pub fn current(&self) -> &Mutex<Option<CancellationToken>> {
        &self.current
    }
}

/// Remove the on-disk artifacts of a download that didn't complete: the
/// incomplete `.partial` stream and any half-written destination file. A failed
/// or cancelled GGUF install must not leave a broken model behind. Idempotent —
/// missing files are ignored.
pub fn cleanup_incomplete_download(dest: &Path) {
    let _ = fs::remove_file(partial_path(dest));
    let _ = fs::remove_file(dest);
}

pub async fn install_hf_gguf_inner(
    app: AppHandle, state: &HfInstallState, endpoint: &str,
    repo: &str, filename: &str, name: &str, dir: PathBuf,
) -> AppResult<()> {
    validate_name(name)?;
    fs::create_dir_all(&dir).map_err(|e| AppError::Io(e.to_string()))?;
    let dest = gguf_dest(&dir, name);

    let token = CancellationToken::new();
    {
        let mut g = state.current.lock_recover();
        if g.is_some() {
            return Err(AppError::Validation("another HF install already in progress".into()));
        }
        *g = Some(token.clone());
    }

    let dl_app = app.clone();
    let on_dl = move |p: DownloadProgress| log_emit(&dl_app, EVENT_HF_PROGRESS, HfPhase::Downloading {
        bytes_completed: p.bytes_completed, bytes_total: p.bytes_total, speed_bps: p.speed_bps,
    });
    let dl = download_gguf(endpoint, repo, filename, &dest, on_dl, token.clone()).await;
    if token.is_cancelled() {
        *state.current.lock_recover() = None;
        cleanup_incomplete_download(&dest);
        return Err(AppError::Validation("install cancelled".into()));
    }
    if let Err(e) = dl {
        // A failed download yields no usable model — drop the incomplete file so
        // nothing broken lingers and a retry starts clean, and free the
        // single-install slot so the next attempt isn't blocked.
        cleanup_incomplete_download(&dest);
        *state.current.lock_recover() = None;
        return Err(e);
    }

    // The downloaded GGUF sits in the shared weights folder — that IS the install.
    let _ = fs::remove_file(partial_path(&dest)); // keep `dest`; only the resume marker is transient
    log_emit(&app, EVENT_MODELS_CHANGED, ());
    *state.current.lock_recover() = None;
    Ok(())
}

#[tauri::command]
pub async fn install_hf_gguf(
    app: AppHandle,
    state: tauri::State<'_, HfInstallState>,
    settings: tauri::State<'_, UserSettingsState>,
    repo: String, filename: String, name: String,
) -> Result<(), AppError> {
    let dir = settings.weights_dir(&app)?;
    install_hf_gguf_inner(app, state.inner(), HF_ENDPOINT, &repo, &filename, &name, dir).await
}

#[tauri::command]
pub fn cancel_hf_install(state: tauri::State<'_, HfInstallState>) -> Result<(), AppError> {
    if let Some(token) = state.current.lock_recover().take() {
        token.cancel();
    }
    Ok(())
}

#[cfg(test)]
#[path = "hf_install_tests.rs"]
mod tests;
