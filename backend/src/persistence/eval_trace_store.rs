use crate::errors::{AppError, AppResult};
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::eval::toolcall::eval::{TaskTrace, TraceResult};
use crate::persistence::evals::sanitize_name;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Same 1 MB read guard as the history store — a corrupt/huge trace file can't
/// OOM the process. Traces hold raw model output so they're bulkier than
/// summaries, but one collection's latest run stays well under this.
pub const MAX_BYTES: u64 = 1024 * 1024;

/// Every model's most-recent per-task traces for one collection. The cache the
/// pipeline visualizer reads so "View Trace" never re-runs inference.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct StoredCollectionTraces {
    pub models: Vec<ModelTraces>,
}

/// One model's traces for the collection, keyed within by task id.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ModelTraces {
    pub model: String,
    pub backend: BackendKind,
    pub tasks: Vec<TaskTrace>,
}

fn store_path(dir: &Path, collection_id: &str) -> AppResult<PathBuf> {
    Ok(dir.join(format!("{}.json", sanitize_name(collection_id)?)))
}

/// The collection's stored traces, or an empty set when nothing has been saved
/// yet (a missing file is not an error).
fn load(dir: &Path, collection_id: &str) -> AppResult<StoredCollectionTraces> {
    Ok(StoredCollectionTraces { models: load_split(dir, collection_id)?.0 })
}

/// The store split into (models this build can interpret, models it can't — kept
/// as raw JSON). Deserializing each model INDEPENDENTLY means one entry written
/// against a different backend set can't make the whole trace cache unreadable,
/// which would otherwise break the visualizer *and* block new trace writes for
/// the collection. The raw remainder is carried back into `upsert`'s rewrite so
/// a legacy entry is never silently deleted.
#[allow(clippy::type_complexity)]
fn load_split(dir: &Path, collection_id: &str) -> AppResult<(Vec<ModelTraces>, Vec<Value>)> {
    let path = store_path(dir, collection_id)?;
    if !path.exists() {
        return Ok((Vec::new(), Vec::new()));
    }
    let len = std::fs::metadata(&path)?.len();
    if len > MAX_BYTES {
        return Err(AppError::Validation(format!(
            "trace file is too large ({len} bytes > {MAX_BYTES} cap)"
        )));
    }
    #[derive(Deserialize)]
    struct RawStore {
        #[serde(default)]
        models: Vec<Value>,
    }
    let content = std::fs::read_to_string(&path)?;
    let raw: RawStore = serde_json::from_str(&content)?;
    let (mut ok, mut bad) = (Vec::new(), Vec::new());
    for v in raw.models {
        match serde_json::from_value::<ModelTraces>(v.clone()) {
            Ok(m) => ok.push(m),
            Err(_) => bad.push(v),
        }
    }
    Ok((ok, bad))
}

/// Save `traces` for `(collection_id, model)`, merging by task id: an existing
/// task's trace is replaced, others kept. Works for a whole-collection write (the
/// Matrix hands the full vec) AND incremental one-task writes (the Simulator
/// streams a task at a time). The model's `backend` is refreshed each call.
pub fn upsert(
    dir: &Path,
    collection_id: &str,
    model: &str,
    backend: BackendKind,
    traces: &[TaskTrace],
) -> AppResult<()> {
    let (mut models, unreadable) = load_split(dir, collection_id)?;
    let store = &mut StoredCollectionTraces { models: std::mem::take(&mut models) };
    let entry = match store.models.iter_mut().find(|m| m.model == model) {
        Some(m) => {
            m.backend = backend;
            m
        }
        None => {
            store.models.push(ModelTraces { model: model.to_string(), backend, tasks: Vec::new() });
            store.models.last_mut().expect("just pushed")
        }
    };
    for t in traces {
        match entry.tasks.iter_mut().find(|x| x.id == t.id) {
            Some(existing) => *existing = t.clone(),
            None => entry.tasks.push(t.clone()),
        }
    }
    std::fs::create_dir_all(dir)?;
    // Re-emit the entries we couldn't interpret alongside the ones we could, so a
    // rewrite never destroys a record this build merely didn't understand.
    let mut out: Vec<Value> = Vec::with_capacity(store.models.len() + unreadable.len());
    for m in &store.models {
        out.push(serde_json::to_value(m)?);
    }
    out.extend(unreadable);
    let json = serde_json::to_string_pretty(&serde_json::json!({ "models": out }))?;
    std::fs::write(store_path(dir, collection_id)?, json)?;
    Ok(())
}

/// The saved trace for one `(collection, model, task)`, or `None` if it was never
/// run/saved — so the visualizer can fall back to a live run.
pub fn load_one(
    dir: &Path,
    collection_id: &str,
    model: &str,
    task_id: &str,
) -> AppResult<Option<TraceResult>> {
    let store = load(dir, collection_id)?;
    Ok(store
        .models
        .into_iter()
        .find(|m| m.model == model)
        .and_then(|m| m.tasks.into_iter().find(|t| t.id == task_id))
        .map(|t| t.trace))
}

#[cfg(test)]
#[path = "eval_trace_store_tests.rs"]
mod tests;
