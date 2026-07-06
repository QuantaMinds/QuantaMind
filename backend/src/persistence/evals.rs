use crate::errors::{AppError, AppResult};
use crate::inference::eval::agentic::v2::collection::load_v2_collection;
use crate::inference::eval::toolcall::tasks::{validate_tasks, ToolTask};
use std::path::{Path, PathBuf};

/// Cap a collection read so a mis-picked giant file can't OOM the process.
pub const MAX_BYTES: u64 = 1024 * 1024;

/// A collection name must be a bare file stem — no separators or traversal —
/// so it can only ever resolve inside the managed `evals/` directory.
pub fn sanitize_name(name: &str) -> AppResult<String> {
    let trimmed = name.trim();
    let bad = trimmed.is_empty()
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("..")
        || trimmed.starts_with('.');
    if bad {
        return Err(AppError::Validation(format!("invalid collection name: {name:?}")));
    }
    Ok(trimmed.to_string())
}

fn collection_path(dir: &Path, name: &str) -> AppResult<PathBuf> {
    Ok(dir.join(format!("{}.json", sanitize_name(name)?)))
}

/// Custom collection names (file stems of `*.json`) in `dir`, sorted. A missing
/// directory is simply an empty registry, not an error.
pub fn list(dir: &Path) -> AppResult<Vec<String>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "json") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                names.push(stem.to_string());
            }
        }
    }
    names.sort();
    Ok(names)
}

/// Read a file to a `String` with the size cap enforced first, so a mis-picked
/// giant file can't OOM the process. The raw-text primitive behind both the JSON
/// collection reader and the CSV-import reader (the frontend never reads files).
pub fn read_text_capped(path: &Path) -> AppResult<String> {
    let len = std::fs::metadata(path)?.len();
    if len > MAX_BYTES {
        return Err(AppError::Validation(format!(
            "file is too large ({len} bytes > {MAX_BYTES} cap)"
        )));
    }
    Ok(std::fs::read_to_string(path)?)
}

/// Read a `.json` task collection from an arbitrary path: size-cap first, then
/// parse and validate. Shared by `load` (managed dir) and the import command
/// (external path) so the cap and the trust boundary are enforced once.
pub fn read_capped(path: &Path) -> AppResult<Vec<ToolTask>> {
    parse_collection(&read_text_capped(path)?)
}

/// Parse a collection from JSON text, detecting the format by the **top-level shape** (an
/// unambiguous discriminator): a JSON **array** is a raw `ToolTask[]`; a JSON **object** is a v2
/// collection (`environment`/`world_state`/`expected_calls`/`target_state`), transpiled via
/// `load_v2_collection`. An object that isn't valid v2 returns `load_v2_collection`'s clear error
/// — it NEVER falls through to the raw-array path. Both paths run the `validate_tasks` trust
/// boundary (v2 inside `load_v2_collection`). Anything else (string/number/null) is unrecognized.
pub fn parse_collection(content: &str) -> AppResult<Vec<ToolTask>> {
    let value: serde_json::Value = serde_json::from_str(content)?; // malformed JSON → InvalidTaskSchema
    match value {
        serde_json::Value::Array(_) => {
            let tasks: Vec<ToolTask> = serde_json::from_value(value)?;
            validate_tasks(&tasks)?;
            Ok(tasks)
        }
        serde_json::Value::Object(_) => load_v2_collection(content),
        _ => Err(AppError::InvalidTaskSchema(
            "unrecognized collection format: expected a JSON array of tasks or a v2 collection object".into(),
        )),
    }
}

pub fn load(dir: &Path, name: &str) -> AppResult<Vec<ToolTask>> {
    read_capped(&collection_path(dir, name)?)
}

pub fn save(dir: &Path, name: &str, tasks: &[ToolTask]) -> AppResult<()> {
    validate_tasks(tasks)?;
    // Semantic trust boundary (write-side only — `load` stays permissive so a
    // pre-existing broken file can still be opened and fixed): a world-state task
    // whose answer key is unreachable or leaky is rejected HERE, naming the exact
    // defect, instead of burning a live eval batch that fails identically every run.
    let findings = crate::inference::eval::agentic::v2::oracle::semantic_findings(tasks);
    if !findings.is_empty() {
        let lines: Vec<String> = findings.iter().map(ToString::to_string).collect();
        return Err(AppError::InvalidTaskSchema(format!(
            "collection has broken world-state answer keys:\n{}",
            lines.join("\n")
        )));
    }
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(tasks)?;
    std::fs::write(collection_path(dir, name)?, json)?;
    Ok(())
}

pub fn delete(dir: &Path, name: &str) -> AppResult<()> {
    let path = collection_path(dir, name)?;
    if !path.exists() {
        return Err(AppError::NotFound(format!("collection '{name}'")));
    }
    std::fs::remove_file(path)?;
    Ok(())
}

#[cfg(test)]
#[path = "evals_tests.rs"]
mod tests;
