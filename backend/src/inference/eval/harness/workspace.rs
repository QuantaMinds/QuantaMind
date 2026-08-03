use crate::errors::{AppError, AppResult};
use crate::inference::eval::mcp::world::{write_seed, McpSpec};
use crate::os::ScratchDir;
use std::path::{Path, PathBuf};

/// Namespaces certify scratch dirs so the orphan sweep never considers an MCP
/// world's directories, and vice versa.
pub const SCRATCH_PREFIX: &str = "qm-cert";

/// The db file name inside the graded root, for db worlds.
const DB_FILE: &str = "data.db";

/// One attempt's isolated workspace.
///
/// Layout is deliberate:
///
/// ```text
/// <scratch>/
/// ├── task.json    ← ours; the agent is told its path; NEVER graded
/// ├── otel/        ← exporter output; NEVER graded
/// └── workspace/   ← THE GRADED ROOT; the agent's cwd; the seed lands here
///     └── data.db  (db worlds)
/// ```
///
/// Nothing of ours sits inside the graded root, so "end state" means exactly the
/// seed plus what the agent did — no third party. The whole tree is removed when
/// the `ScratchDir` drops.
pub struct Workspace {
    _scratch: ScratchDir,
    root: PathBuf,
    task_file: PathBuf,
    otel_dir: PathBuf,
    db: Option<PathBuf>,
}

impl Workspace {
    /// Build and seed a fresh workspace for one attempt.
    ///
    /// A fresh directory per attempt is not tidiness — it is the precondition for
    /// pass^k meaning anything. If attempt 2 inherited attempt 1's world, the
    /// oracle would grade leftovers and k runs would not be independent.
    pub fn seed(spec: &McpSpec, task_id: &str, goal: &str) -> AppResult<Workspace> {
        let scratch = ScratchDir::new(SCRATCH_PREFIX)?;
        let root = scratch.path().join("workspace");
        let otel_dir = scratch.path().join("otel");
        std::fs::create_dir_all(&root).map_err(|e| AppError::Io(e.to_string()))?;
        std::fs::create_dir_all(&otel_dir).map_err(|e| AppError::Io(e.to_string()))?;

        let db = match spec {
            McpSpec::Fs { seed, .. } => {
                // `write_seed` already confines every path through `fs_guard` after
                // rejecting `..`/absolute, so a malformed seed cannot escape.
                write_seed(&root, seed)?;
                None
            }
            McpSpec::Db { seed, .. } => {
                let db_path = root.join(DB_FILE);
                // An empty `setup_sql` is a legitimate world ("the agent creates the
                // schema"), but sqlite3 still needs a statement to materialize the
                // file. Same substitution `McpWorld` makes, so both paths agree.
                let sql = if seed.setup_sql.trim().is_empty() { "SELECT 1;" } else { &seed.setup_sql };
                crate::inference::eval::mcp::oracle_db::run_sqlite(&db_path, sql)?;
                Some(db_path)
            }
        };

        // Written OUTSIDE the graded root — see the type doc.
        let task_file = scratch.path().join("task.json");
        let payload = serde_json::json!({ "id": task_id, "goal": goal });
        std::fs::write(&task_file, serde_json::to_vec_pretty(&payload).unwrap_or_default())
            .map_err(|e| AppError::Io(e.to_string()))?;

        Ok(Workspace { _scratch: scratch, root, task_file, otel_dir, db })
    }

    /// The graded root — the agent's cwd, and the only thing the oracle reads.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn task_file(&self) -> &Path {
        &self.task_file
    }

    pub fn otel_dir(&self) -> &Path {
        &self.otel_dir
    }

    pub fn db(&self) -> Option<&Path> {
        self.db.as_deref()
    }
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;
