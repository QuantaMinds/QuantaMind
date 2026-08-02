//! `qm.json` — the tiny run config `qm init` writes and `qm run` reads for its
//! defaults, so a second run needs zero flags typed. Lives in the cwd; nothing
//! secret goes in it (a remote key stays in env/keychain, never this file).

use crate::errors::{AppError, AppResult};
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::eval::costs::CostConfig;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The config file name, resolved relative to the cwd.
pub const CONFIG_FILE: &str = "qm.json";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct QmConfig {
    pub backend: BackendKind,
    pub model: String,
    pub collection: String,
    pub profile: String,
    /// Endpoint override for remote backends (never a key — that stays in env/keychain).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// Cost basis for `--costs`. Omitted entirely by `qm init` — there is no
    /// default price, and a guessed one would understate a real bill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub costs: Option<CostConfig>,
}

impl QmConfig {
    /// Load `./qm.json` if present and valid. `None` when absent or unparseable
    /// (a broken config should not crash `qm run` — the user just passes flags).
    pub fn load(dir: &Path) -> Option<QmConfig> {
        let bytes = std::fs::read(dir.join(CONFIG_FILE)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Write `./qm.json` (pretty). Returns the relative path for a user-facing
    /// message — never an absolute path (rule 7f).
    pub fn save(&self, dir: &Path) -> AppResult<PathBuf> {
        let path = dir.join(CONFIG_FILE);
        let json = serde_json::to_string_pretty(self).map_err(|e| AppError::Internal(e.to_string()))?;
        std::fs::write(&path, json).map_err(|e| AppError::Internal(format!("write {CONFIG_FILE}: {e}")))?;
        Ok(PathBuf::from(CONFIG_FILE))
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
