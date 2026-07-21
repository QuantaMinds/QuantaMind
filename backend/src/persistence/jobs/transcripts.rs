use crate::errors::{AppError, AppResult};
use crate::inference::eval::agentic::step::TrajectoryStep;
use crate::inference::eval::batch::TaskOutcome;
use crate::persistence::at_rest::{at_rest, AtRest};
use crate::persistence::readiness::safe_filename::safe_filename;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// A deep pass^k transcript (k runs × steps × raw model output + env snapshots)
/// can be large; cap reads so a runaway file can't OOM the process.
pub const MAX_READ_BYTES: u64 = 32 * 1024 * 1024;

/// One `.jsonl` line: a live turn or the task's terminal outcome (always last).
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum TranscriptRecord<'a> {
    Step(&'a TrajectoryStep),
    Outcome(&'a TaskOutcome),
}

/// The OWNED mirror of `TranscriptRecord` for reading a transcript back —
/// `qm costs` and the app's disk history parse persisted runs through this.
/// Kept byte-compatible with the writer by the round-trip test.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptEntry {
    Step(TrajectoryStep),
    Outcome(TaskOutcome),
}

/// Parse a transcript file into its records. A torn FINAL line is tolerated (a
/// crash mid-append leaves one — the completed records before it are still
/// good); a malformed line anywhere ELSE is corruption and errors loudly, never
/// a silent partial read (docs/architecture.md#robustness).
pub fn load_records(path: &Path) -> AppResult<Vec<TranscriptEntry>> {
    let text = read(path)?;
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut out = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        match serde_json::from_str::<TranscriptEntry>(line) {
            Ok(rec) => out.push(rec),
            Err(e) if i + 1 == lines.len() => {
                println!("[transcripts] WARN: dropping torn final line of {} ({e})", path.display());
                break;
            }
            Err(e) => {
                return Err(AppError::Internal(format!(
                    "corrupt transcript {} at line {}: {e}",
                    path.display(),
                    i + 1
                )))
            }
        }
    }
    Ok(out)
}

/// The transcript path for one (collection, model, task, pass):
/// `<dir>/<collection>/<model>--<task>[--native].jsonl`. Every segment goes
/// through `safe_filename` (model names carry `:` and `/`; ids can be long) so
/// the path is collision-proof and path-safe. `dir` is
/// `app_config_dir/agentic_transcripts` — deliberately NOT `transcripts/`,
/// which belongs to the STT feature.
pub fn transcript_path(dir: &Path, collection_id: &str, model: &str, task_id: &str, native: bool) -> PathBuf {
    let pass = if native { "--native" } else { "" };
    dir.join(safe_filename(collection_id))
        .join(format!("{}--{}{}.jsonl", safe_filename(model), safe_filename(task_id), pass))
}

/// Start a task's transcript: truncate/create. Retention is LATEST BATCH ONLY —
/// re-running a (collection, model, task) replaces its transcript, so disk stays
/// bounded by collection size × models (the `eval_trace_store` philosophy).
pub fn begin_task(path: &Path) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    File::create(path)?;
    Ok(())
}

/// Append one live turn — an O(1) OS-atomic append (the `queue::append` pattern),
/// so a crash mid-append can only truncate the trailing line.
pub fn append_step(path: &Path, step: &TrajectoryStep) -> AppResult<()> {
    append_record(path, &TranscriptRecord::Step(step))
}

/// Append the task's terminal outcome — the last line of a completed transcript.
pub fn append_outcome(path: &Path, outcome: &TaskOutcome) -> AppResult<()> {
    append_record(path, &TranscriptRecord::Outcome(outcome))
}

fn append_record(path: &Path, record: &TranscriptRecord<'_>) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new().append(true).create(true).open(path)?;
    // Encryption-at-rest seam (no-op in the OSS build; see persistence::at_rest). Passthrough
    // returns the line unchanged, so this is byte-identical to a plain writeln today.
    let line = serde_json::to_string(record)?;
    let sealed = at_rest().seal(line.as_bytes());
    f.write_all(&sealed)?;
    f.write_all(b"\n")?;
    f.flush()?;
    Ok(())
}

/// The raw transcript text, size-capped. Parsing is the reader's concern — each
/// line is a self-describing tagged record (`{"step": …}` / `{"outcome": …}`).
pub fn read(path: &Path) -> AppResult<String> {
    let len = std::fs::metadata(path)?.len();
    if len > MAX_READ_BYTES {
        return Err(AppError::Validation(format!(
            "transcript too large ({len} bytes > {MAX_READ_BYTES} cap)"
        )));
    }
    Ok(std::fs::read_to_string(path)?)
}

#[cfg(test)]
#[path = "transcripts_tests.rs"]
mod tests;
