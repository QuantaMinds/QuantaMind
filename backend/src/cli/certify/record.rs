//! Record a suite by *demonstrating* it, instead of hand-writing the answer key.
//!
//! Authoring an oracle by hand is the step that stops teams adopting a gate: you
//! must know, in advance and in assertion form, exactly what correct looks like.
//! `--record` inverts that. Run the agent you already trust once, and QuantaMind
//! writes down what it actually did to the world.
//!
//! **State-only, deliberately.** This records the structural delta — files created
//! and deleted — and nothing else. Content assertions are NOT auto-generated: a
//! recorded body would embed timestamps, ids and run-specific text and produce a
//! suite that fails on its second run for reasons that have nothing to do with the
//! agent. Modified files are reported for the human to assert on, which is a much
//! smaller job than writing the whole oracle.

use crate::inference::eval::mcp::world::McpSpec;
use std::collections::BTreeMap;
use std::path::Path;

/// What one task's world looked like at a moment in time: relative path → content.
pub type Snapshot = BTreeMap<String, Vec<u8>>;

/// The delta a recorded run observed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Delta {
    /// Present after, absent before → the agent made it.
    pub created: Vec<String>,
    /// Present before, absent after → the agent removed it.
    pub deleted: Vec<String>,
    /// Present in both with different bytes. Reported, never auto-asserted.
    pub modified: Vec<String>,
}

impl Delta {
    /// True when the agent changed nothing. A recording of nothing cannot become
    /// a test — the resulting oracle would pass on an empty run.
    pub fn is_empty(&self) -> bool {
        self.created.is_empty() && self.deleted.is_empty()
    }
}

/// Walk a graded root into a snapshot. Symlinks are not followed: a recorded suite
/// must describe the world, not wherever a link happens to point on this machine.
pub fn snapshot(root: &Path) -> Snapshot {
    let mut out = Snapshot::new();
    walk(root, root, &mut out);
    out
}

fn walk(root: &Path, dir: &Path, out: &mut Snapshot) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        let Ok(meta) = e.metadata() else { continue };
        if meta.is_symlink() {
            continue;
        }
        if meta.is_dir() {
            walk(root, &path, out);
        } else if let Ok(rel) = path.strip_prefix(root) {
            // Forward slashes so a suite recorded on Windows replays on Unix.
            let key = rel.to_string_lossy().replace('\\', "/");
            if let Ok(bytes) = std::fs::read(&path) {
                out.insert(key, bytes);
            }
        }
    }
}

/// The seed as a snapshot, so before/after are compared in the same terms.
pub fn seed_snapshot(spec: &McpSpec) -> Snapshot {
    match spec {
        McpSpec::Fs { seed, .. } => {
            seed.files.iter().map(|(p, c)| (p.replace('\\', "/"), c.clone().into_bytes())).collect()
        }
        McpSpec::Db { .. } => Snapshot::new(),
    }
}

/// Compare the seed against the end state.
pub fn diff(before: &Snapshot, after: &Snapshot) -> Delta {
    let mut d = Delta::default();
    for (p, bytes) in after {
        match before.get(p) {
            None => d.created.push(p.clone()),
            Some(was) if was != bytes => d.modified.push(p.clone()),
            Some(_) => {}
        }
    }
    for p in before.keys() {
        if !after.contains_key(p) {
            d.deleted.push(p.clone());
        }
    }
    d
}

/// Turn a delta into the oracle fields of a suite task.
///
/// Only `assert_present` and `assert_absent` — see the module doc for why content
/// is left to the human.
pub fn to_oracle_json(d: &Delta) -> serde_json::Value {
    serde_json::json!({
        "assert_present": d.created,
        "assert_absent": d.deleted,
    })
}

/// Whether a world can be recorded at all.
///
/// Db worlds cannot yet: a meaningful delta needs a schema-aware comparison, and
/// emitting a plausible-looking SQL oracle we have not actually verified would be
/// worse than emitting nothing.
pub fn unsupported_reason(spec: &McpSpec) -> Option<&'static str> {
    match spec {
        McpSpec::Fs { .. } => None,
        McpSpec::Db { .. } => Some(
            "db worlds cannot be recorded yet — a correct SQL oracle needs a schema-aware diff, \
             and a guessed one would be worse than none. Write the assert_eq/assert_contains by hand.",
        ),
    }
}

/// The banner every recorded file carries. A recording is taken from a real run,
/// so it can contain real data.
pub const REVIEW_BANNER: &str =
    "RECORDED by `qm certify --record`. This was captured from a real run and may contain real \
     data — review before committing. Content assertions are NOT auto-generated; add them for any \
     file listed under `_modified`.";

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
