//! The world-manager: a fresh, disposable sandbox with a REAL MCP server scoped
//! to it. Per run — seed → spawn-scoped → execute → **grade end-state** →
//! teardown — with a fresh directory every time (the precondition for pass^k
//! independence). Isolation is the safety here: the server is confined to the
//! sandbox, and it's thrown away, so controlled-world runs need no human
//! approval gate (that's for the user's REAL tools — see P9).

use crate::errors::{AppError, AppResult};
use crate::inference::mcp::bridge::{execute_call, ToolExecution};
use crate::inference::ollama::ollama_chat::NativeToolCall;
use crate::mcp::client::McpClient;
use crate::persistence::fs_guard;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The starting files of a filesystem world (relative path → contents). We author
/// this, so we know the correct end-state — that's the answer key.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FsSeed {
    pub files: BTreeMap<String, String>,
}

impl FsSeed {
    pub fn from<const N: usize>(files: [(&str, &str); N]) -> FsSeed {
        FsSeed { files: files.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }
    }
}

/// A sqlite world seed: SQL run once (via the `sqlite3` CLI) to build the
/// initial DB. We author it, so the correct end-state is knowable.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DbSeed {
    pub setup_sql: String,
}

impl DbSeed {
    pub fn new(sql: &str) -> DbSeed {
        DbSeed { setup_sql: sql.to_string() }
    }
}

/// Write a seed into `root`, confining every path (rejects `..`/absolute, and
/// `fs_guard` resolves symlinks) so a malformed seed can't escape the sandbox.
pub fn write_seed(root: &Path, seed: &FsSeed) -> AppResult<()> {
    for (rel, contents) in &seed.files {
        if rel.contains("..") || Path::new(rel).is_absolute() {
            return Err(AppError::Validation(format!("seed path '{rel}' must be relative, no ..")));
        }
        let target = root.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
        }
        let confined = fs_guard::ensure_within(root, &target)?;
        std::fs::write(&confined, contents).map_err(|e| AppError::Io(e.to_string()))?;
    }
    Ok(())
}

/// A fresh scratch directory, removed on drop. Avoids the `tempfile` dev-dep in
/// production; uniqueness from pid + a process-lifetime counter.
struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    fn new() -> AppResult<ScratchDir> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("qm-mcp-world-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|e| AppError::Io(e.to_string()))?;
        Ok(ScratchDir { path })
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A controlled world: a fresh sandbox + a real MCP filesystem server scoped to
/// only that sandbox. Field order matters — `client` (→ server) drops before
/// `scratch` (→ dir removal), so the dir is free of open handles when removed.
pub struct McpWorld {
    client: McpClient,
    /// The CANONICAL root the server confines to. Drivers must build paths from
    /// this (not the raw scratch path) so they match the server's allowed dir —
    /// on macOS `/var/folders/…` canonicalizes to `/private/var/folders/…`.
    root: PathBuf,
    scratch: ScratchDir,
}

impl McpWorld {
    /// Seed a fresh temp dir and start `@modelcontextprotocol/server-filesystem`
    /// scoped to it. A new call = a new, byte-identical-from-seed world.
    pub async fn filesystem(seed: &FsSeed) -> AppResult<McpWorld> {
        let scratch = ScratchDir::new()?;
        write_seed(&scratch.path, seed)?;
        // The server confines the model to this canonical root (EscapeRoute-safe).
        let root = scratch.path.canonicalize().map_err(|e| AppError::Io(e.to_string()))?;
        let client = McpClient::connect(
            "npx",
            &["-y".into(), "@modelcontextprotocol/server-filesystem".into(), root.to_string_lossy().into_owned()],
            "quantamind-world",
            env!("CARGO_PKG_VERSION"),
        )
        .await?;
        Ok(McpWorld { client, root, scratch })
    }

    /// Seed a fresh temp sqlite DB and start `mcp-server-sqlite-npx` pointed at
    /// it — the second world type, proving the world/oracle abstraction
    /// generalizes. A new call = a new world seeded from the same SQL.
    pub async fn sqlite(seed: &DbSeed) -> AppResult<McpWorld> {
        use crate::inference::eval::mcp::oracle_db::run_sqlite;
        let scratch = ScratchDir::new()?;
        let root = scratch.path.canonicalize().map_err(|e| AppError::Io(e.to_string()))?;
        let db = root.join("data.db");
        // Non-empty seed builds the schema+rows; empty still materializes the file.
        let sql = if seed.setup_sql.trim().is_empty() { "SELECT 1;" } else { &seed.setup_sql };
        run_sqlite(&db, sql)?;
        let client = McpClient::connect(
            "npx",
            &["-y".into(), "mcp-server-sqlite-npx".into(), db.to_string_lossy().into_owned()],
            "quantamind-world",
            env!("CARGO_PKG_VERSION"),
        )
        .await?;
        Ok(McpWorld { client, root, scratch })
    }

    /// The canonical sandbox root (the per-run temp dir). Build tool-call paths
    /// from this so they match the server's allowed directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The sqlite DB path inside the sandbox (for a `sqlite` world).
    pub fn db_path(&self) -> PathBuf {
        self.root.join("data.db")
    }

    pub fn client(&self) -> &McpClient {
        &self.client
    }

    /// Execute one tool call against the world's server (the responder seam a
    /// driver calls per step). Result is inert text; `is_error` is rechecked.
    pub async fn execute(&self, call: &NativeToolCall) -> AppResult<ToolExecution> {
        execute_call(&self.client, call).await
    }

    /// Kill the server now (idempotent). The dir is removed when the world drops.
    pub fn teardown(&self) {
        self.client.kill();
    }
}

impl Drop for McpWorld {
    fn drop(&mut self) {
        // Kill the server first so the dir has no open handles when it's removed.
        self.client.kill();
    }
}

#[cfg(test)]
#[path = "world_tests.rs"]
mod tests;
