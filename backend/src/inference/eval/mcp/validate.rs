//! World validation — proving an authored MCP world is a RELIABLE test before any
//! model runs in it. Two layers:
//!
//! 1. [`static_world_findings`] — pure authoring-contract checks (vacuous or
//!    contradictory oracles, escaping seed paths). Instant, no environment.
//! 2. [`validate_world_live`] — spawn the REAL world and run the **do-nothing
//!    check**: grade the oracle against the untouched, freshly-seeded world with
//!    zero agent actions. It must FAIL — if doing nothing passes, the task is
//!    vacuous and every pass^k it ever produces is a lie. (The failure mode is not
//!    hypothetical: 2026 τ-bench audits measured a do-nothing agent at 38% pass^k;
//!    SWE-bench had to ship "Verified" for the same reason.)
//!
//! The deep sandbox validator (`v2::oracle::validate_collection_deep`) proves
//! reachable+discriminating for deterministic worlds but returns `not_checkable`
//! for `RequireWorldOracle` tasks — this module closes that hole for MCP worlds.

use crate::errors::AppResult;
use crate::inference::eval::mcp::world::{McpSpec, McpWorld};
use crate::redact::redact_path;

/// Outcome of the live world check for one spec.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldLiveCheck {
    /// The world spawned (seed written, MCP server up, handshake done).
    pub spawns: bool,
    /// Redacted reason when it didn't.
    pub spawn_error: Option<String>,
    /// `Some(true)` = the do-nothing agent FAILS the oracle (good — the task
    /// discriminates). `Some(false)` = the untouched seed already satisfies the
    /// oracle (vacuous task). `None` when the world never spawned.
    pub discriminating: Option<bool>,
}

/// Pure authoring-contract findings for a world spec (Error severity — each one
/// makes the task untrustworthy). Empty vec = statically sound.
pub fn static_world_findings(spec: &McpSpec) -> Vec<String> {
    let mut out = Vec::new();
    match spec {
        McpSpec::Fs { seed, oracle } => {
            if oracle.assert_present.is_empty() && oracle.assert_absent.is_empty() && oracle.assert_content.is_empty() {
                out.push("vacuous oracle: no assertions at all — every run would pass regardless of what the model does".into());
            }
            for p in &oracle.assert_present {
                if oracle.assert_absent.contains(p) {
                    out.push(format!("contradictory oracle: '{p}' is asserted both present and absent — unsatisfiable"));
                }
            }
            for (p, _) in &oracle.assert_content {
                if oracle.assert_absent.contains(p) {
                    out.push(format!("contradictory oracle: '{p}' must contain content but is also asserted absent"));
                }
            }
            for rel in seed.files.keys() {
                if crate::inference::eval::mcp::world::is_unsafe_seed_path(rel) {
                    // Authoring-time version of the runtime write_seed guard, redacted.
                    // Shares the exact predicate so static + runtime never disagree.
                    out.push(format!("seed path '{}' must be relative with no '..' (it would escape the sandbox)", redact_path(rel)));
                }
            }
        }
        McpSpec::Db { seed: _, oracle } => {
            // An empty setup_sql is deliberately NOT a finding: "model creates the
            // schema" is a legitimate world. Only an oracle that asserts nothing is.
            if oracle.assert_contains.is_empty() && oracle.assert_eq.is_empty() {
                out.push("vacuous oracle: no assertions at all — every run would pass regardless of what the model does".into());
            }
        }
    }
    out
}

/// Spawn the real world and run the do-nothing check. A spawn failure is reported
/// in the result (redacted), not returned as `Err` — the caller decides whether it
/// is a world defect or missing machine deps (npx/sqlite3).
pub async fn validate_world_live(spec: &McpSpec) -> AppResult<WorldLiveCheck> {
    match McpWorld::from_spec(spec).await {
        Err(e) => Ok(WorldLiveCheck { spawns: false, spawn_error: Some(redact_path(&e.to_string())), discriminating: None }),
        Ok(world) => {
            // Zero agent actions: grade the untouched seed. Passing here means the
            // task can be "solved" by doing nothing — vacuous.
            let do_nothing_passes = world.grade(spec);
            world.teardown();
            Ok(WorldLiveCheck { spawns: true, spawn_error: None, discriminating: Some(!do_nothing_passes) })
        }
    }
}

/// The machine deps world tasks need, when any are missing: `npx` (Node) for every
/// MCP server; `sqlite3` additionally for Db worlds. Returns the user-facing fix
/// (exact install pointer), or `None` when the machine can run these worlds.
pub fn world_deps_missing(specs: &[&McpSpec]) -> Option<String> {
    if specs.is_empty() {
        return None;
    }
    use crate::os::{EngineHost, Host};
    let have = |bin: &str| {
        // Via `Host::command` per the repo's disallowed-`Command::new` lint
        // (applies CREATE_NO_WINDOW so a probe never flashes a console on Windows).
        Host::command(bin)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
    };
    let mut missing = Vec::new();
    if !have("npx") {
        missing.push("npx (install Node.js — https://nodejs.org)");
    }
    if specs.iter().any(|s| matches!(s, McpSpec::Db { .. })) && !have("sqlite3") {
        missing.push("sqlite3 (macOS: preinstalled/brew install sqlite · Linux: apt install sqlite3 · Windows: sqlite.org/download)");
    }
    if missing.is_empty() {
        None
    } else {
        Some(format!("world tasks need: {}", missing.join(" · ")))
    }
}

/// Fold world checks into a `CollectionValidation` — the ONE composition both the
/// CLI (`qm validate` + the run gate) and the GUI validate commands call, so the
/// app's existing validation display shows world findings with no frontend change:
/// static findings land in each task's `semantic` (Error) list; the live check maps
/// onto the same `reachable`/`discriminating` fields the deep validator uses.
/// `live` = spawn each world and run the do-nothing check (skipped when deps are
/// missing — reported as a collection-level structural note instead of a guess).
pub async fn merge_world_checks(
    v: &mut crate::inference::eval::agentic::v2::oracle::CollectionValidation,
    tasks: &[crate::inference::eval::toolcall::tasks::ToolTask],
    live: bool,
) {
    let world_specs: Vec<&McpSpec> = tasks.iter().filter_map(|t| t.agentic.as_ref().and_then(|a| a.mcp.as_ref())).collect();
    if world_specs.is_empty() {
        return;
    }
    let deps_missing = world_deps_missing(&world_specs);

    for t in tasks {
        let Some(spec) = t.agentic.as_ref().and_then(|a| a.mcp.as_ref()) else { continue };
        let Some(tv) = v.tasks.iter_mut().find(|tv| tv.id == t.id) else { continue };

        let static_findings = static_world_findings(spec);
        let statically_broken = !static_findings.is_empty();
        for f in static_findings {
            tv.semantic.push(format!("world: {f}"));
        }

        if let Some(dep) = &deps_missing {
            tv.detail = format!("world not live-checked — {dep}");
        } else if live && !statically_broken {
            match validate_world_live(spec).await {
                Ok(check) => {
                    tv.reachable = if check.spawns { "yes".into() } else { "no".into() };
                    tv.discriminating = check.discriminating;
                    if let Some(err) = check.spawn_error {
                        tv.semantic.push(format!("world failed to spawn: {err} — fix the seed/server, then re-validate"));
                    } else if check.discriminating == Some(false) {
                        tv.semantic.push(
                            "world: the untouched seed already satisfies the oracle — a do-nothing agent passes, so this task \
                             proves nothing. Make the oracle assert a CHANGE the model must cause (e.g. a new file/row)."
                                .into(),
                        );
                    } else {
                        tv.detail = "world spawns · do-nothing agent fails the oracle (discriminating)".into();
                    }
                }
                Err(e) => tv.semantic.push(format!("world live-check errored: {}", redact_path(&e.to_string()))),
            }
        }
    }
    // Recompute the collection verdict with the world findings folded in.
    v.ok = v.tasks.iter().all(|t| t.reachable != "no" && t.discriminating != Some(false) && t.semantic.is_empty());
}

#[cfg(test)]
#[path = "validate_tests.rs"]
mod tests;
