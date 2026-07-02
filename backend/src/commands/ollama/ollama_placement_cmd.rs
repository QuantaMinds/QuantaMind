use crate::errors::AppError;
use crate::inference::backend::endpoint;
use crate::inference::ollama::ollama_placement::{probe_placement, ModelPlacement};
use serde::Serialize;

/// UI-facing placement of a loaded Ollama model: whether it spilled onto the CPU and a
/// ready-to-show `note`. `None` when the model isn't loaded or Ollama isn't reachable — the
/// UI then shows nothing (never a fabricated state).
#[derive(Serialize)]
pub struct PlacementDto {
    pub on_cpu: bool,
    pub total_bytes: u64,
    pub vram_bytes: u64,
    pub cpu_bytes: u64,
    /// A user-facing line ("Running on CPU… giving each step more time"); `None` when resident.
    pub note: Option<String>,
}

impl From<ModelPlacement> for PlacementDto {
    fn from(p: ModelPlacement) -> Self {
        PlacementDto {
            on_cpu: p.on_cpu,
            total_bytes: p.total_bytes,
            vram_bytes: p.vram_bytes,
            cpu_bytes: p.cpu_bytes,
            note: p.note(),
        }
    }
}

/// Probe how Ollama placed `model` (VRAM vs CPU) so the eval UI can warn the user that a
/// CPU-offloaded run is slow (and that the harness gives it more time). Best-effort: returns
/// `None` if the model isn't loaded / Ollama is down.
#[tauri::command]
pub async fn ollama_model_placement(model: String) -> Result<Option<PlacementDto>, AppError> {
    Ok(probe_placement(endpoint::OLLAMA, &model).await.map(PlacementDto::from))
}
