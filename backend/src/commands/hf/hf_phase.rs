use serde::Serialize;

pub const EVENT_HF_PROGRESS: &str = "hf-progress";

/// Progress of a Hugging Face GGUF install. The install is a single download
/// into the shared weights folder, so `Downloading` is the only phase — there is
/// no post-download import step to report.
#[derive(Serialize, Clone)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum HfPhase {
    Downloading { bytes_completed: u64, bytes_total: u64, speed_bps: u64 },
}
