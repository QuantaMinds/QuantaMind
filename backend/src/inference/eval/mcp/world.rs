//! The world-manager: a fresh, disposable sandbox with a REAL MCP server scoped
//! to it. Per run — seed → spawn-scoped → execute → **grade end-state** →
//! teardown — with a fresh directory every time (the precondition for pass^k
//! independence). Isolation is the safety here: the server is confined to the
//! sandbox, and it's thrown away, so controlled-world runs need no human
//! approval gate (that's for the user's REAL tools — see P9).

use crate::errors::{AppError, AppResult};
use crate::inference::eval::mcp::oracle_db::DbOracle;
use crate::inference::eval::mcp::oracle_fs::FsOracle;
use crate::inference::mcp::bridge::{execute_call, ToolExecution};
use crate::inference::ollama::ollama_chat::NativeToolCall;
use crate::mcp::client::McpClient;
use crate::fs_guard;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The immutable spec for an MCP controlled world carried on an agentic task
/// (`ResponderKind::Mcp`): the seed we author + the oracle answer key. Serde so it
/// rides inside a `ToolTask`'s `AgenticSpec`. The server is spawned per-run in the
/// runner (like `WebUiSpec`), never here.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpSpec {
    Fs { seed: FsSeed, oracle: FsOracle },
    Db { seed: DbSeed, oracle: DbOracle },
}

/// The agentic runner speaks `Call{name,args}`; the MCP client speaks
/// `NativeToolCall{name,args}` — a field-for-field convert.
impl From<crate::inference::eval::toolcall::tasks::Call> for NativeToolCall {
    fn from(c: crate::inference::eval::toolcall::tasks::Call) -> Self {
        NativeToolCall { name: c.name, args: c.args }
    }
}

/// The starting files of a filesystem world (relative path → contents). We author
/// this, so we know the correct end-state — that's the answer key.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FsSeed {
    #[serde(default)]
    pub files: BTreeMap<String, String>,
}

impl FsSeed {
    pub fn from<const N: usize>(files: [(&str, &str); N]) -> FsSeed {
        FsSeed { files: files.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }
    }
}

/// A sqlite world seed: SQL run once (via the `sqlite3` CLI) to build the
/// initial DB. We author it, so the correct end-state is knowable.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DbSeed {
    #[serde(default)]
    pub setup_sql: String,
}

impl DbSeed {
    pub fn new(sql: &str) -> DbSeed {
        DbSeed { setup_sql: sql.to_string() }
    }
}

/// Is a seed path unsafe to write under the sandbox? Platform-INDEPENDENT (a world
/// authored on Unix must be rejected identically when validated on Windows and vice
/// versa — `Path::is_absolute()` alone disagrees across platforms: a Unix `/etc/x`
/// is not "absolute" on Windows). Unsafe = traversal (`..`), a POSIX/Windows root
/// (`/` or `\` lead), a Windows drive prefix (`C:`), or the platform's own
/// `is_absolute`.
pub fn is_unsafe_seed_path(rel: &str) -> bool {
    let bytes = rel.as_bytes();
    rel.contains("..")
        || matches!(bytes.first(), Some(b'/') | Some(b'\\'))
        || (bytes.len() >= 2 && bytes[1] == b':') // C:\ or C:/ drive prefix
        || Path::new(rel).is_absolute()
}

/// Write a seed into `root`, confining every path (rejects `..`/absolute, and
/// `fs_guard` resolves symlinks) so a malformed seed can't escape the sandbox.
pub fn write_seed(root: &Path, seed: &FsSeed) -> AppResult<()> {
    for (rel, contents) in &seed.files {
        if is_unsafe_seed_path(rel) {
            // Redacted (rule 7f): an ABSOLUTE seed path is exactly the case where the
            // offending string can carry /Users/<name>/… — never echo it verbatim.
            return Err(AppError::Validation(format!(
                "seed path '{}' must be relative, no ..",
                crate::redact::redact_path(rel)
            )));
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
        sweep_orphans();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("qm-mcp-world-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|e| AppError::Io(e.to_string()))?;
        Ok(ScratchDir { path })
    }
}

/// Best-effort sweep of `qm-mcp-world-<pid>-*` dirs whose owning process is DEAD —
/// a SIGKILL'd run can never Drop, so without this every hard kill leaks a temp
/// dir forever. Unix-only (`kill -0` liveness probe); errors ignored (a sweep must
/// never block a new world). Called once per world construction — cheap, since the
/// scan only pays when orphans actually exist.
fn sweep_orphans() {
    #[cfg(unix)]
    {
        use crate::os::{EngineHost, Host};
        let me = std::process::id();
        let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else { return };
        for e in entries.flatten() {
            let name = e.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(rest) = name.strip_prefix("qm-mcp-world-") else { continue };
            let Some(pid_str) = rest.split('-').next() else { continue };
            let Ok(pid) = pid_str.parse::<u32>() else { continue };
            if pid == me {
                continue; // our own live worlds
            }
            // `kill -0` = liveness probe, sends no signal. Non-success → pid is dead
            // (or not ours to signal — either way its worlds are not in use by us).
            // Via `Host::command` per the repo's disallowed-`Command::new` lint.
            let alive = Host::command("kill")
                .args(["-0", &pid.to_string()])
                .status()
                .map(|s| s.success())
                .unwrap_or(true); // probe failed → assume alive, never sweep in doubt
            if !alive {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
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
    /// RAII guard: kept solely so its `Drop` removes the temp dir on teardown.
    #[allow(dead_code)]
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

    /// Build the world for an [`McpSpec`] (fs or db) — the per-run construction the
    /// agentic runner calls (async, like `WebUiState::from_spec` but real).
    pub async fn from_spec(spec: &McpSpec) -> AppResult<McpWorld> {
        match spec {
            McpSpec::Fs { seed, .. } => McpWorld::filesystem(seed).await,
            McpSpec::Db { seed, .. } => McpWorld::sqlite(seed).await,
        }
    }

    /// Grade the world's end-state against the spec's oracle (τ-bench: the world,
    /// not the words). This is what `EndStateRule::RequireWorldOracle` reads.
    pub fn grade(&self, spec: &McpSpec) -> bool {
        match spec {
            McpSpec::Fs { oracle, .. } => oracle.grade(self.root()).passed,
            McpSpec::Db { oracle, .. } => oracle.grade(&self.db_path()).passed,
        }
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
