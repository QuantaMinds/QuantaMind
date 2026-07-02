//! Where Ollama placed a loaded model's weights — how much sits in VRAM/GPU vs spilled onto
//! system RAM (CPU). A model that doesn't fully fit is offloaded; CPU inference runs several
//! times slower, which (a) needs a larger per-step timeout so a slow turn isn't killed as a
//! false `TurnTimeout`, and (b) should be surfaced to the user so a long run reads as expected,
//! not stuck. Read from Ollama's `/api/ps`.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// A loaded model's weight placement. `on_cpu` is true only for a MEANINGFUL spill (>5% on the
/// CPU) — a few-percent rounding difference between `size` and `size_vram` is normal for a
/// fully-resident model and must not read as "running on CPU".
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ModelPlacement {
    pub total_bytes: u64,
    pub vram_bytes: u64,
    pub cpu_bytes: u64,
    pub on_cpu: bool,
}

impl ModelPlacement {
    /// A short user-facing line describing how the model is running (and, when offloaded, that
    /// the eval gives it more time). `None` when fully resident — nothing worth interrupting the
    /// user for.
    pub fn note(&self) -> Option<String> {
        if !self.on_cpu {
            return None;
        }
        let cpu_gb = self.cpu_bytes as f64 / 1_000_000_000.0;
        let pct = (self.cpu_bytes as f64 / self.total_bytes.max(1) as f64 * 100.0).round();
        Some(if self.vram_bytes == 0 {
            "Running on CPU (no GPU offload) — inference is much slower; the eval is giving each step more time.".to_string()
        } else {
            format!("Running partly on CPU ({cpu_gb:.1} GB / {pct:.0}% didn't fit in GPU memory) — slower; the eval is giving each step more time.")
        })
    }
}

#[derive(Deserialize)]
struct PsResponse {
    #[serde(default)]
    models: Vec<PsModel>,
}

#[derive(Deserialize)]
struct PsModel {
    #[serde(default)]
    name: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    size_vram: u64,
}

/// Parse an `/api/ps` body for `model`'s placement. Matches on either `name` or `model` (Ollama
/// echoes both; a caller may pass the tag or the resolved id). `None` when the model isn't
/// currently loaded (nothing to report) or its size is unknown. Pure — tested without a server.
pub fn parse_placement(body: &str, model: &str) -> Option<ModelPlacement> {
    let ps: PsResponse = serde_json::from_str(body).ok()?;
    let m = ps.models.into_iter().find(|m| m.name == model || m.model == model)?;
    if m.size == 0 {
        return None;
    }
    let vram = m.size_vram.min(m.size);
    let cpu = m.size.saturating_sub(vram);
    Some(ModelPlacement {
        total_bytes: m.size,
        vram_bytes: vram,
        cpu_bytes: cpu,
        // Meaningful spill only: >5% of the weights on the CPU (cpu * 20 > total).
        on_cpu: cpu.saturating_mul(20) > m.size,
    })
}

/// Probe Ollama's `/api/ps` for `model`'s placement. Best-effort: any failure (server down,
/// model not loaded) returns `None`, so a caller degrades to the resident-model behavior.
pub async fn probe_placement(endpoint: &str, model: &str) -> Option<ModelPlacement> {
    let client = Client::builder().timeout(PROBE_TIMEOUT).build().ok()?;
    let resp = client.get(format!("{endpoint}/api/ps")).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    parse_placement(&resp.text().await.ok()?, model)
}

#[cfg(test)]
#[path = "ollama_placement_tests.rs"]
mod tests;
