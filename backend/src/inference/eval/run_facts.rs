//! Post-run stamping of measured host facts onto a `BatchReport`'s columns — the ONE
//! implementation both frontends use (the Tauri batch command and the `qm` CLI), so the
//! app's Latency Test-run view and `qm … --costs` can never drift. Only facts that are
//! measurable WITHOUT Tauri state live here (the Ollama `/api/ps` placement); the
//! llama-server launch facts (kv type, GGUF bytes) stay in the command layer — they come
//! from the app-managed `LlamaServerState`, which a CLI run doesn't have (an externally
//! managed server's flags are unknowable, and stay honestly unstamped).

use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::eval::batch::BatchColumn;
use crate::inference::eval::toolcall::matrix::ModelTarget;
use crate::inference::ollama::ollama_placement::{probe_placement, ModelPlacement};
use std::collections::HashMap;

/// Probe `/api/ps` once per Ollama target for its weight placement. Best-effort: an
/// unreachable server / unloaded model simply isn't in the map (nothing to report).
pub async fn probe_placements(
    targets: &[ModelTarget],
    endpoint_for: impl Fn(BackendKind) -> String,
) -> HashMap<String, ModelPlacement> {
    let mut placements = HashMap::new();
    for t in targets {
        if t.backend == BackendKind::Ollama {
            if let Some(p) = probe_placement(&endpoint_for(t.backend), &t.model).await {
                placements.insert(t.model.clone(), p);
            }
        }
    }
    placements
}

/// Stamp the probed placement facts onto the report columns: measured weight split
/// (`weights_total/vram_bytes`, `offload_bytes` = size − size_vram — the "why 3 tok/s"
/// quantity), the meaningful-spill bool, and the tag's CLAIMED quantization.
pub fn stamp_placements(columns: &mut [BatchColumn], placements: &HashMap<String, ModelPlacement>) {
    for col in columns {
        if let Some(p) = placements.get(&col.model) {
            col.cpu_offloaded = p.on_cpu;
            col.weights_total_bytes = Some(p.total_bytes);
            col.weights_vram_bytes = Some(p.vram_bytes);
            col.offload_bytes = Some(p.cpu_bytes);
            col.quantization_claimed = p.quantization_claimed.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamping_writes_only_probed_models_and_all_placement_facts() {
        let mut cols = vec![
            BatchColumn { model: "probed".into(), ..Default::default() },
            BatchColumn { model: "unprobed".into(), ..Default::default() },
        ];
        let mut placements = HashMap::new();
        placements.insert(
            "probed".to_string(),
            ModelPlacement {
                total_bytes: 10_000_000_000,
                vram_bytes: 6_000_000_000,
                cpu_bytes: 4_000_000_000,
                on_cpu: true,
                quantization_claimed: Some("Q4_K_M".into()),
            },
        );
        stamp_placements(&mut cols, &placements);
        assert_eq!(cols[0].weights_total_bytes, Some(10_000_000_000));
        assert_eq!(cols[0].weights_vram_bytes, Some(6_000_000_000));
        assert_eq!(cols[0].offload_bytes, Some(4_000_000_000));
        assert!(cols[0].cpu_offloaded);
        assert_eq!(cols[0].quantization_claimed.as_deref(), Some("Q4_K_M"));
        // An unprobed model stays all-None — absence of a probe is never a measurement.
        assert_eq!(cols[1].weights_total_bytes, None);
        assert!(!cols[1].cpu_offloaded);
    }
}
