use crate::errors::{AppError, AppResult};
use crate::inference::eval::readiness::types::CliffStatus;
use crate::persistence::readiness::safe_filename::safe_filename;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Many models × a small record each — a generous guard against a corrupt giant file.
pub const MAX_BYTES: u64 = 256 * 1024;

/// The cliff file for a collection. `safe_filename` sanitizes ONLY the file name;
/// the model keys inside the map are stored verbatim (Ollama names carry colons).
fn cliff_path(dir: &Path, collection_id: &str) -> PathBuf {
    dir.join(format!("{}.json", safe_filename(collection_id)))
}

/// One value from the on-disk map → a `CliffStatus`. New entries are the tagged
/// object (`{"status":"NoCliff","tested":4000}`); a **legacy bare number** (the old
/// `{ model: cliff_tokens }` format) migrates to `Collapsed { depth }` — the only
/// thing the old store ever recorded was a found collapse depth.
fn status_from_value(v: &Value) -> Option<CliffStatus> {
    match v {
        Value::Number(n) => n.as_u64().map(|d| CliffStatus::Collapsed { depth: d as u32, concentration: None }),
        other => serde_json::from_value(other.clone()).ok(),
    }
}

/// The per-model context-cliff status for a collection — `{ model: CliffStatus }`.
/// Empty map when none saved (a missing file is not an error). Keys are the RAW
/// model name (colons intact). Tolerant of the legacy bare-`u32` format on load.
pub fn load(dir: &Path, collection_id: &str) -> AppResult<HashMap<String, CliffStatus>> {
    let path = cliff_path(dir, collection_id);
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let len = std::fs::metadata(&path)?.len();
    if len > MAX_BYTES {
        return Err(AppError::Validation(format!("cliff file too large ({len} bytes > {MAX_BYTES} cap)")));
    }
    let raw: HashMap<String, Value> = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    Ok(raw.iter().filter_map(|(k, v)| status_from_value(v).map(|s| (k.clone(), s))).collect())
}

/// Record one model's cliff status for a collection (last-write-wins per model).
/// **Atomic**: load → merge → write a temp file → `rename` over the target. A crash
/// mid-write can only leave the inert `.tmp`; the live `.json` is always 100% old or
/// 100% new, never a half-written file that would wipe the collection's history.
pub fn save(dir: &Path, collection_id: &str, model: &str, status: CliffStatus) -> AppResult<()> {
    std::fs::create_dir_all(dir)?;
    let mut map = load(dir, collection_id)?;
    map.insert(model.to_string(), status); // verbatim model key
    let json = serde_json::to_string_pretty(&map)?;
    let final_path = cliff_path(dir, collection_id);
    let tmp_path = final_path.with_extension("json.tmp");
    std::fs::write(&tmp_path, json)?;
    std::fs::rename(&tmp_path, &final_path)?; // OS-atomic swap
    Ok(())
}

#[cfg(test)]
#[path = "cliff_tests.rs"]
mod tests;
