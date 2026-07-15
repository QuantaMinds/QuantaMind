//! The **filesystem-state oracle** (Track B): grade a world's END-STATE against
//! an answer key we authored — files present/absent, contents. This is the whole
//! anti-fake-done point: we check what actually changed on disk, never the
//! model's claim ("I created it" is not a pass; the file existing is).

use serde::{Deserialize, Serialize};
use std::path::Path;

/// The answer key for a filesystem world. Paths are relative to the sandbox root.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FsOracle {
    #[serde(default)]
    pub assert_present: Vec<String>,
    #[serde(default)]
    pub assert_absent: Vec<String>,
    /// `(relative path, substring the file must contain)`.
    #[serde(default)]
    pub assert_content: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FsVerdict {
    pub passed: bool,
    pub failures: Vec<String>,
}

impl FsOracle {
    /// Grade the end-state of the world rooted at `root`.
    pub fn grade(&self, root: &Path) -> FsVerdict {
        let mut failures = Vec::new();
        for p in &self.assert_present {
            if !root.join(p).exists() {
                failures.push(format!("expected present but missing: {p}"));
            }
        }
        for p in &self.assert_absent {
            if root.join(p).exists() {
                failures.push(format!("expected absent but still present: {p}"));
            }
        }
        for (p, needle) in &self.assert_content {
            match std::fs::read_to_string(root.join(p)) {
                Ok(c) if c.contains(needle) => {}
                Ok(_) => failures.push(format!("{p} does not contain expected content: {needle:?}")),
                Err(_) => failures.push(format!("{p} unreadable for content check")),
            }
        }
        FsVerdict { passed: failures.is_empty(), failures }
    }
}

#[cfg(test)]
#[path = "oracle_fs_tests.rs"]
mod tests;
