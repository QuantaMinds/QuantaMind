//! The **DB-state oracle** (Track B, second world type — proves the world/oracle
//! abstraction generalizes beyond the filesystem). Grades a sqlite world's
//! end-state by running SELECTs **directly via the `sqlite3` CLI**, independent
//! of the agent's tools (τ-bench reads the DB itself) — so it can't be fooled by
//! the model's claims. (Windows needs `sqlite3.exe` on PATH.)

use crate::errors::{AppError, AppResult};
use crate::os::{EngineHost, Host};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Run SQL against `db` via the `sqlite3` CLI; returns the trimmed stdout.
pub(crate) fn run_sqlite(db: &Path, sql: &str) -> AppResult<String> {
    let out = Host::command("sqlite3")
        .arg(db)
        .arg(sql)
        .output()
        .map_err(|e| AppError::Io(format!("sqlite3 spawn: {e}")))?;
    if !out.status.success() {
        return Err(AppError::Internal(format!("sqlite3: {}", String::from_utf8_lossy(&out.stderr))));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The answer key for a sqlite world.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DbOracle {
    /// `(SELECT query, substring the trimmed result must contain)`.
    #[serde(default)]
    pub assert_contains: Vec<(String, String)>,
    /// `(SELECT query, exact trimmed result — e.g. a COUNT)`.
    #[serde(default)]
    pub assert_eq: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DbVerdict {
    pub passed: bool,
    pub failures: Vec<String>,
}

impl DbOracle {
    pub fn grade(&self, db: &Path) -> DbVerdict {
        let mut failures = Vec::new();
        for (q, needle) in &self.assert_contains {
            match run_sqlite(db, q) {
                Ok(r) if r.contains(needle) => {}
                Ok(r) => failures.push(format!("`{q}` = {r:?}, expected to contain {needle:?}")),
                Err(e) => failures.push(format!("`{q}` failed: {e}")),
            }
        }
        for (q, expected) in &self.assert_eq {
            match run_sqlite(db, q) {
                Ok(r) if &r == expected => {}
                Ok(r) => failures.push(format!("`{q}` = {r:?}, expected {expected:?}")),
                Err(e) => failures.push(format!("`{q}` failed: {e}")),
            }
        }
        DbVerdict { passed: failures.is_empty(), failures }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn sqlite3_available() -> bool {
        Host::command("sqlite3").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
    }

    fn temp_db() -> std::path::PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir()
            .join(format!("qm-dboracle-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("t.db")
    }

    #[test]
    fn grades_a_seeded_db_directly_via_the_cli() {
        if !sqlite3_available() {
            eprintln!("skip: no sqlite3 CLI");
            return;
        }
        let db = temp_db();
        run_sqlite(&db, "CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT); INSERT INTO users(name) VALUES('Bob');").unwrap();

        let pass = DbOracle {
            assert_contains: vec![("SELECT name FROM users;".into(), "Bob".into())],
            assert_eq: vec![("SELECT COUNT(*) FROM users;".into(), "1".into())],
        };
        assert!(pass.grade(&db).passed);

        let fail = DbOracle {
            assert_eq: vec![("SELECT COUNT(*) FROM users WHERE name='Alice';".into(), "1".into())],
            ..Default::default()
        };
        let v = fail.grade(&db);
        assert!(!v.passed, "no Alice row yet");
        assert!(v.failures[0].contains("Alice"));

        std::fs::remove_dir_all(db.parent().unwrap()).ok();
    }
}
