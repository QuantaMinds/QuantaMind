use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

/// Tauri-free client for llama-server's `/props` — the ONLY way to learn the window of a
/// server the app didn't spawn (a manual launch, or one the `qm` CLI points at). Lives in
/// the domain so both the GUI commands and the lean CLI can preflight against the REAL
/// running window instead of discovering it mid-ladder. Best-effort: every failure
/// degrades to `None`, never a guess.

/// The subset of `/props` we surface: the loaded model's path and the launch context.
#[derive(Deserialize)]
struct LlamaProps {
    /// Absolute GGUF path (recent llama.cpp). Older builds omit it, carrying the path
    /// in `default_generation_settings.model` instead — handled by the fallback below.
    #[serde(default)]
    model_path: String,
    #[serde(default)]
    default_generation_settings: PropsGenSettings,
}

#[derive(Deserialize, Default)]
struct PropsGenSettings {
    #[serde(default)]
    n_ctx: u32,
    #[serde(default)]
    model: String,
}

/// Extract `(model_path, n_ctx)` from a `/props` body. Prefers the top-level
/// `model_path`; falls back to the per-slot `model` field older builds report.
/// `None` when neither names a model (nothing usable is loaded).
pub fn parse_props(body: &str) -> Option<(String, u32)> {
    let props: LlamaProps = serde_json::from_str(body).ok()?;
    let path = if !props.model_path.is_empty() {
        props.model_path
    } else if !props.default_generation_settings.model.is_empty() {
        props.default_generation_settings.model.clone()
    } else {
        return None;
    };
    Some((path, props.default_generation_settings.n_ctx))
}

/// Probe `endpoint`'s `/props` for `(model_path, n_ctx)`. `None` when nothing is
/// listening, `/props` fails, or no model path is reported.
pub async fn probe_props(endpoint: &str, timeout_ms: u64) -> Option<(String, u32)> {
    let client = Client::builder().timeout(Duration::from_millis(timeout_ms)).build().ok()?;
    let resp = client.get(format!("{endpoint}/props")).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    parse_props(&resp.text().await.ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modern_and_legacy_props_shapes() {
        let modern = r#"{"model_path":"/w/m.gguf","default_generation_settings":{"n_ctx":12288}}"#;
        assert_eq!(parse_props(modern), Some(("/w/m.gguf".into(), 12288)));
        let legacy = r#"{"default_generation_settings":{"n_ctx":8192,"model":"/w/old.gguf"}}"#;
        assert_eq!(parse_props(legacy), Some(("/w/old.gguf".into(), 8192)));
        assert_eq!(parse_props(r#"{"default_generation_settings":{"n_ctx":4096}}"#), None, "no model ⇒ None");
        assert_eq!(parse_props("not json"), None);
    }
}
