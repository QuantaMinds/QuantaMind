use crate::errors::{AppError, AppResult};
use crate::inference::eval::agentic::spec::Tier;
use crate::inference::eval::batch::CompletedUnit;
use crate::inference::eval::toolcall::matrix::ModelTarget;
use crate::inference::eval::toolcall::tasks::ToolTask;
use crate::persistence::prompts::schema::InferenceParams;
use crate::persistence::readiness::safe_filename::safe_filename;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// A resumable run can hold a lot of units (models × tasks) with full task specs
/// in the header — a generous cap that still guards against a corrupt giant file.
pub const MAX_BYTES: u64 = 32 * 1024 * 1024;

/// Everything needed to reconstruct the work-list on resume, so the job log is a
/// self-contained source of truth (no dependence on the frontend re-sending it).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RunConfig {
    pub collection_id: String,
    pub targets: Vec<ModelTarget>,
    pub tasks: Vec<ToolTask>,
    pub k: Option<u32>,
    pub max_steps: Option<u32>,
    pub params: Option<InferenceParams>,
    pub keep_alive: Option<i32>,
    /// Run the NATIVE tool-calling pass (the OpenAI tool wire tool_calls).
    pub native: bool,
    /// Run the PROMPT-based pass (JSON-in-text proxy). At least one of `native`/`prompt` is set by
    /// the UI. `#[serde(default = "default_true")]` → a job log written before this field resumes
    /// with the prior behavior, where the prompt pass always ran.
    #[serde(default = "default_true")]
    pub prompt: bool,
    /// Phase 9: the difficulty tier the run was launched at. `None` is the legacy
    /// (Custom / free-`k`) path. `#[serde(default)]` keeps pre-Phase-9 job logs
    /// resumable — an interrupted run written before this field still parses.
    #[serde(default)]
    pub tier: Option<Tier>,
    /// Phase 9 anti-saturation: decoy-tool budget injected per agentic task. `None`
    /// leaves each task's authored decoys untouched. `#[serde(default)]` for the
    /// same back-compat reason as `tier`.
    #[serde(default)]
    pub decoy_tools: Option<u32>,
    /// The user-chosen thinking-budget preset (D8 sidebar). Drives the per-turn scratchpad budget
    /// for reasoning models (`think_tokens_for_preset`). `#[serde(default)]` = `Standard`, so a job
    /// log written before this field resumes at the default preset. Stamped onto the verdict so a
    /// result is only ever compared to others produced under the SAME preset (reproducibility).
    #[serde(default)]
    pub think_preset: crate::inference::eval::agentic::difficulty::passk::ThinkPreset,
    /// PR2: the inner task-concurrency knob (tasks-within-a-model run at once). `None` → serial
    /// (N=1), byte-identical to the pre-concurrency dispatcher. `#[serde(default)]` keeps an
    /// interrupted job log written before this field resumable — it deserializes to `None`.
    #[serde(default)]
    pub eval_concurrency: Option<usize>,
}

/// serde default for `RunConfig::prompt` — an interrupted job written before the field existed
/// always ran the prompt pass, so it resumes as `true`.
fn default_true() -> bool {
    true
}

/// One `.jsonl` line: the run header (first line) or a completed unit.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum JobRecord {
    Header(RunConfig),
    Unit(CompletedUnit),
}

/// The log path for a run — keyed by the collision-proof filename so a long/nested
/// collection id can't overflow or collide.
pub fn run_path(dir: &Path, run_id: &str) -> PathBuf {
    dir.join(format!("{}.jsonl", safe_filename(run_id)))
}

/// Start a fresh log: (over)write the file with just the header line.
pub fn create(path: &Path, config: &RunConfig) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = File::create(path)?; // truncates any stale log for this run
    writeln!(f, "{}", serde_json::to_string(&JobRecord::Header(config.clone()))?)?;
    f.flush()?;
    Ok(())
}

/// Append one completed unit — an O(1) OS-atomic append (NOT a read-modify-write),
/// so a crash mid-append can only truncate the trailing line (which `load` heals).
pub fn append(path: &Path, unit: &CompletedUnit) -> AppResult<()> {
    let mut f = OpenOptions::new().append(true).create(true).open(path)?;
    writeln!(f, "{}", serde_json::to_string(&JobRecord::Unit(unit.clone()))?)?;
    f.flush()?;
    Ok(())
}

/// Load a run's header + completed units, **healing a truncated tail**: a final
/// half-written JSON line (from a hard crash mid-append) is silently discarded —
/// that unit is simply treated as not-done and re-runs. `None` when there's no
/// usable header (no resumable run).
pub fn load(path: &Path) -> AppResult<Option<(RunConfig, Vec<CompletedUnit>)>> {
    if !path.exists() {
        return Ok(None);
    }
    let len = std::fs::metadata(path)?.len();
    if len > MAX_BYTES {
        return Err(AppError::Validation(format!("job log too large ({len} bytes > {MAX_BYTES} cap)")));
    }
    let mut header: Option<RunConfig> = None;
    let mut units = Vec::new();
    for line in BufReader::new(File::open(path)?).lines() {
        let Ok(line) = line else { break }; // I/O error on the tail → stop
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<JobRecord>(&line) {
            Ok(JobRecord::Header(c)) => header = Some(c),
            Ok(JobRecord::Unit(u)) => units.push(u),
            Err(_) => break, // truncated/corrupt final line → discard + stop (heal)
        }
    }
    Ok(header.map(|c| (c, units)))
}

pub fn delete(path: &Path) -> AppResult<()> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Every run log present in `dir` (a leftover log == an interrupted run).
pub fn list_paths(dir: &Path) -> AppResult<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "jsonl") {
            out.push(path);
        }
    }
    Ok(out)
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
