//! Answer-key validation for authored collections — a "does my task actually work?" check
//! the user runs BEFORE spending a real model on it. It proves each task is
//! (a) *reachable* — a perfect "oracle" agent that replays the checkpoint calls reaches the
//! end state — and (b) *discriminating* — a do-nothing agent FAILS it. A task that fails (a)
//! has a broken answer key (a typo'd checkpoint tool, a wildcard that never matches, a required
//! call with no mock); one that fails (b) is trivially passable and measures nothing. Both are
//! authoring bugs the user could otherwise mistake for "every model is bad at my task".
//!
//! This is the same oracle the build-time satisfiability test uses for the bundled collections,
//! lifted into a reusable path so a custom/imported collection gets the identical proof.

use crate::errors::AppResult;
use crate::inference::eval::agentic::build::sandbox_for;
use crate::inference::eval::agentic::model_turn::{ModelTurn, Progress};
use crate::inference::eval::agentic::runner::run_once;
use crate::inference::eval::agentic::sandbox::{EndStateRule, TaskCheckpoint};
use crate::inference::eval::agentic::spec::{FaultInjection, SafetyArm};
use crate::inference::eval::agentic::v2::world_state::{derive_response, ACK, RESERVED};
use crate::inference::eval::toolcall::tasks::{is_agentic, Call, ToolTask};
use crate::inference::generate::generate_spec::GenerateSpec;
use crate::inference::generate::generate_stats::GenerateStats;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc::unbounded_channel;

/// A canned agent: emits `calls[i]` on turn `i`, then a no-op `{}` (no tool call) forever.
/// The whole `ModelTurn` seam — no backend, so validation needs no server or model.
struct Scripted {
    calls: Vec<String>,
    next: AtomicUsize,
}

impl ModelTurn for Scripted {
    async fn run(&self, _s: &GenerateSpec, _progress: &Progress) -> AppResult<(String, GenerateStats)> {
        let i = self.next.fetch_add(1, Ordering::SeqCst);
        let body = self.calls.get(i).cloned().unwrap_or_else(|| "{}".into());
        Ok((body, GenerateStats { eval_count: Some(1), ..Default::default() }))
    }
}

/// Replace each `*…*` wildcard string with a concrete value that satisfies the glob (its
/// literal segments joined); everything else stays exact. Lets the oracle satisfy a wildcard
/// checkpoint the way a correct model would (a real value that matches the pattern).
fn concretize(v: &Value) -> Value {
    match v {
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), concretize(x))).collect()),
        Value::String(s) if s.contains('*') => {
            // Strip a leading `~` (unordered sigil) so the concretized value is the bare
            // tokens joined — the ordered/unordered distinction is the matcher's job.
            let lit: String = s.trim_start_matches('~').split('*').filter(|p| !p.is_empty()).collect();
            Value::String(if lit.is_empty() { "x".into() } else { lit })
        }
        other => other.clone(),
    }
}

/// The kind of semantic defect found in an authored task's answer key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticFindingKind {
    /// A digit-bearing `world_state` entity id named in neither the prompt nor any
    /// other entity's blob — the model has no path to it (every run stalls).
    OrphanEntity,
    /// An expected getter call that `derive_response` can't resolve to real data —
    /// the model calls the right tool, learns nothing, and decides blind.
    AckingGetter,
    /// A non-RESERVED `world_state` key no intended path fetches — pure oracle data
    /// a lucky arg guess would exfiltrate whole.
    UnfetchedKey,
    /// A glob literal an expected action/reporter checkpoint demands that appears
    /// in neither the prompt, any presented tool name, nor any data an earlier
    /// expected call surfaces — the model must GUESS the grader's exact wording,
    /// which manufactures false-negative fails on capable models.
    UngroundedAnswerToken,
    /// Two required getters resolve to the SAME entity and one exposes a superset of
    /// (or the same fields as) the other — the bigger getter already hands the model
    /// everything the smaller was required to fetch, so a model that reads efficiently
    /// and skips the smaller call is falsely failed as "missing" it. The default
    /// whole-blob responder makes every unprojected sibling getter redundant; the fix is
    /// a per-tool `returns_fields` slice (or dropping/retagging the redundant checkpoint).
    LeakyGetter,
    /// Category K: an Attack-arm safety probe with no `must_not_call` trap — there is no
    /// forbidden action to resist, so the probe can never be failed and its "resistance"
    /// is meaningless (not discriminating).
    SafetyAttackNoTrap,
    /// Category K: an Attack-arm probe whose guard marker is absent from the prompt — the
    /// runner can't tell whether the guard survived, so a failure can't be attributed.
    SafetyGuardNotInPrompt,
    /// Category K: a BenignControl-arm probe that carries a `must_not_call` trap — a benign
    /// control must be SAFE to complete, so trapping it manufactures a fake over-refusal.
    SafetyBenignTrapped,
    /// Category K: a BenignControl-arm probe graded as abstain (no legitimate call to make)
    /// — a benign control's correct behavior is to PROCEED, so it must require the call;
    /// otherwise there is no over-refusal to measure.
    SafetyBenignNoLegitPath,
}

/// How certain a finding is, which decides its enforcement. The first three
/// checks are mechanical certainties → `Error` (hard-block at `evals::save`).
/// Answer grounding is a HEURISTIC — the audit that motivated it produced false
/// positives needing human triage (a calc `*` that meant multiplication, words
/// grounded in a tool name) — so it is `Warning`: surfaced with evidence at
/// authoring/import time, never a silent block. The guard proposes; the author
/// disposes. (Bundled scenarios are still held to zero warnings by CI.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticSeverity {
    Error,
    Warning,
}

impl SemanticFindingKind {
    pub fn severity(self) -> SemanticSeverity {
        match self {
            // Heuristic authoring smells (surfaced, never hard-blocked): the answer key MIGHT
            // intend the redundant call, so a leaky getter is a Warning like an ungrounded token.
            SemanticFindingKind::UngroundedAnswerToken | SemanticFindingKind::LeakyGetter => SemanticSeverity::Warning,
            _ => SemanticSeverity::Error,
        }
    }
}

/// One semantic defect in an authored task, with a message that names the task and
/// the exact fix. The same checks back the bundled-scenario CI guards
/// (`scenarios.rs`) and the custom-collection save/import trust boundary
/// (`evals::save`) — one implementation, so CI and import can never drift.
#[derive(Debug, Clone)]
pub struct SemanticFinding {
    pub task_id: String,
    pub kind: SemanticFindingKind,
    pub message: String,
}

impl SemanticFinding {
    pub fn severity(&self) -> SemanticSeverity {
        self.kind.severity()
    }
}

impl std::fmt::Display for SemanticFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Whole-word token match, mirroring `generator::replace_ids`' boundary rule: a
/// match is bounded by a non-alphanumeric byte (or the string edge) on both sides,
/// so "M-1" never matches inside "M-12".
fn names_token(text: &str, id: &str) -> bool {
    let alnum = |b: u8| b.is_ascii_alphanumeric();
    let tb = text.as_bytes();
    let mut i = 0;
    while let Some(off) = text[i..].find(id) {
        let (s, e) = (i + off, i + off + id.len());
        let left = s == 0 || !alnum(tb[s - 1]);
        let right = e >= tb.len() || !alnum(tb[e]);
        if left && right {
            return true;
        }
        i = s + 1;
    }
    false
}

/// The machine-checkable world-state authoring contract, over TRANSPILED tasks
/// (the shape custom collections persist as). Empty = clean. Only entity-env
/// agentic tasks with a `world_state` are checked; single-turn, mocks-based, and
/// filesystem/corpus/web-UI tasks pass through untouched.
pub fn semantic_findings(tasks: &[ToolTask]) -> Vec<SemanticFinding> {
    tasks.iter().flat_map(task_semantic_findings).collect()
}

fn task_semantic_findings(task: &ToolTask) -> Vec<SemanticFinding> {
    let mut out = Vec::new();
    let Some(spec) = task.agentic.as_ref() else { return out };
    // Category K discrimination — env-INDEPENDENT (file/search injection use the
    // Filesystem/WebCorpus responders, which the entity-only checks below skip), so it runs
    // FIRST and is preserved through those early returns.
    if let Some(safety) = spec.safety.as_ref() {
        let tid = &task.id;
        let mut push = |kind, message: String| out.push(SemanticFinding { task_id: tid.clone(), kind, message });
        match safety.arm {
            SafetyArm::Attack => {
                if spec.must_not_call.is_empty() {
                    push(
                        SemanticFindingKind::SafetyAttackNoTrap,
                        format!("{tid}: Attack safety probe has no must_not_call trap — there is no forbidden action to resist, so it can never be failed; add the dangerous action as a must_not_call entry"),
                    );
                }
                let marker = safety.guard.marker.trim();
                if marker.is_empty() || !task.prompt.contains(marker) {
                    push(
                        SemanticFindingKind::SafetyGuardNotInPrompt,
                        format!("{tid}: guard marker {:?} is absent from the prompt — attribution can't tell whether the guard survived; plant the marker verbatim in the task prompt/standing policy", safety.guard.marker),
                    );
                }
            }
            SafetyArm::BenignControl => {
                if !spec.must_not_call.is_empty() {
                    push(
                        SemanticFindingKind::SafetyBenignTrapped,
                        format!("{tid}: benign-control probe carries a must_not_call trap — a benign control must be safe to complete; remove the trap or reclassify this as an Attack probe"),
                    );
                }
                if matches!(spec.end_state, EndStateRule::ExpectAbstainingText) {
                    push(
                        SemanticFindingKind::SafetyBenignNoLegitPath,
                        format!("{tid}: benign-control probe grades as abstain — its correct behavior is to PROCEED, so it must require the legitimate call; add expected_calls"),
                    );
                }
            }
        }
    }
    if !spec.environment.is_entity() {
        return out;
    }
    let Some(ws_val) = spec.world_state.as_ref() else { return out };
    let Some(ws) = ws_val.as_object() else { return out };
    let tid = &task.id;
    let mut push = |kind, message: String| {
        out.push(SemanticFinding { task_id: tid.clone(), kind, message });
    };

    let checkpoints: &[TaskCheckpoint] = match &spec.end_state {
        EndStateRule::RequireAll(cps) | EndStateRule::RequireSequence(cps) => cps,
        _ => &[],
    };
    let in_another_blob =
        |key: &str| ws.iter().any(|(k, val)| k != key && names_token(&val.to_string(), key));

    // (1) Entity reachability: every digit-bearing non-reserved id must be named in
    // the prompt (ROOT entity) or inside another entity's blob (DISCOVERED entity).
    for key in ws.keys() {
        let is_entity = !RESERVED.contains(&key.as_str()) && key.chars().any(|c| c.is_ascii_digit());
        if is_entity && !names_token(&task.prompt, key) && !in_another_blob(key) {
            push(
                SemanticFindingKind::OrphanEntity,
                format!("{tid}: world_state entity '{key}' is in neither the prompt nor any other entity's blob — the model has no path to its id; name it in the prompt or in a blob a getter surfaces"),
            );
        }
    }

    // (2) Answer-key data: every expected getter call must resolve to real
    // world_state data, never the generic ack. Reporter tools (the text-bearing
    // reply channel) are exempt: their ack IS the response. Raw args are tried
    // before the glob-concretized form so a calc expression's literal `*`
    // (multiplication) never reads as a false violation.
    let reporters: Vec<&str> = task
        .tools
        .iter()
        .filter(|t| t.parameters.pointer("/properties/text").is_some())
        .map(|t| t.name.as_str())
        .collect();
    for cp in checkpoints {
        let is_getter = spec.entity_tools.iter().any(|g| g == &cp.tool) && !reporters.contains(&cp.tool.as_str());
        if !is_getter {
            continue;
        }
        let resolves = derive_response(ws_val, &Call { name: cp.tool.clone(), args: cp.args.clone() }) != ACK
            || derive_response(ws_val, &Call { name: cp.tool.clone(), args: concretize(&cp.args) }) != ACK;
        if !resolves {
            push(
                SemanticFindingKind::AckingGetter,
                format!("{tid}: expected getter {}({}) resolves to NO world_state data (it acks) — move the fact under a top-level key matching the call's arg value or the tool name", cp.tool, cp.args),
            );
        }
    }

    // (2b) Leaky getter: two required getters resolving to the SAME entity where one's
    // exposed field set contains the other's. The default responder hands back the WHOLE
    // entity blob, so any unprojected sibling getter is redundant — a model that reads
    // efficiently from the bigger getter and skips the smaller is falsely failed for the
    // "missing" call. The exposed set is the getter's `returns_fields` slice if declared,
    // else all (non-reserved) blob keys.
    let exposure = |cp: &TaskCheckpoint| -> Option<(String, std::collections::BTreeSet<String>)> {
        if !spec.entity_tools.iter().any(|g| g == &cp.tool) || reporters.contains(&cp.tool.as_str()) {
            return None;
        }
        let raw = derive_response(ws_val, &Call { name: cp.tool.clone(), args: cp.args.clone() });
        let blob = if raw != ACK { raw } else { derive_response(ws_val, &Call { name: cp.tool.clone(), args: concretize(&cp.args) }) };
        let v: Value = serde_json::from_str(&blob).ok()?;
        let obj = v.as_object()?;
        // Group siblings by the entity read: the first arg value that is a world_state key,
        // else the tool-name blob key.
        let entity = cp
            .args
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(_, x)| x.as_str())
            .find(|s| ws.contains_key(*s))
            .map(str::to_string)
            .unwrap_or_else(|| cp.tool.clone());
        let projected = spec.field_projections.get(&cp.tool);
        let fields: std::collections::BTreeSet<String> = obj
            .keys()
            .filter(|k| !RESERVED.contains(&k.as_str()))
            .filter(|k| projected.map_or(true, |p| p.contains(k)))
            .cloned()
            .collect();
        (!fields.is_empty()).then_some((entity, fields))
    };
    let mut exposures: Vec<(String, String, std::collections::BTreeSet<String>)> = Vec::new();
    for cp in checkpoints {
        if let Some((entity, fields)) = exposure(cp) {
            if !exposures.iter().any(|(e, t, _)| e == &entity && t == &cp.tool) {
                exposures.push((entity, cp.tool.clone(), fields));
            }
        }
    }
    for i in 0..exposures.len() {
        for j in (i + 1)..exposures.len() {
            let (e_i, t_i, f_i) = &exposures[i];
            let (e_j, t_j, f_j) = &exposures[j];
            if e_i != e_j {
                continue;
            }
            let (small, big, leaked) = if f_j.is_subset(f_i) {
                (t_j, t_i, f_j)
            } else if f_i.is_subset(f_j) {
                (t_i, t_j, f_i)
            } else {
                continue;
            };
            let fields: Vec<&str> = leaked.iter().map(String::as_str).collect();
            push(
                SemanticFindingKind::LeakyGetter,
                format!(
                    "{tid}: on entity '{e_i}', getter {big} already exposes all of {small}'s required fields ({}) — a model reading {big} needn't call {small}, so skipping {small} is falsely failed; give {big} a `returns_fields` slice excluding {small}'s fields (or drop/retag the redundant checkpoint)",
                    fields.join(", ")
                ),
            );
        }
    }

    // (3) Leak containment: a non-reserved key no intended path reaches (prompt,
    // another blob, an expected-call arg, or a real tool name) is pure oracle data
    // `derive_response` would hand to any lucky arg guess.
    let mut arg_values: HashSet<String> = HashSet::new();
    for cp in checkpoints {
        for v in cp.args.as_object().into_iter().flatten().map(|(_, v)| v) {
            if let Some(s) = v.as_str() {
                arg_values.insert(s.to_string());
                arg_values.insert(s.trim_matches('*').to_string());
            }
        }
    }
    for key in ws.keys() {
        if RESERVED.contains(&key.as_str()) {
            continue;
        }
        let fetchable = names_token(&task.prompt, key)
            || in_another_blob(key)
            || arg_values.contains(key.as_str())
            || spec.recognized_tools.iter().any(|t| t == key);
        if !fetchable {
            push(
                SemanticFindingKind::UnfetchedKey,
                format!("{tid}: world_state key '{key}' is oracle data no intended call fetches, yet any arg guess equal to '{key}' would be handed its whole blob — house it under a reserved meta key (e.g. \"outcome\", \"ground_truth\") or make it reachable"),
            );
        }
    }

    // (4) Answer grounding: a checkpoint glob is the grader's WORDING — every
    // literal segment an expected ACTION/REPORTER call demands must be teachable:
    // present in the prompt, a presented tool name, or data an EARLIER expected
    // call surfaces. An ungrounded token means a capable model that did everything
    // right still fails because it phrased the answer in its own words. The corpus
    // is accumulated in expected-call order, mirroring what a perfect agent has
    // actually seen by that step (getter responses via `derive_response`, raw args
    // first so a calc expression's literal `*` is never mangled). Matching is
    // separator-tolerant (`normalize_grounding`) so "work-product" grounds
    // "work product" — a hyphen variant is a wording match, not a missing word.
    let mut corpus = normalize_grounding(&task.prompt);
    for t in &task.tools {
        corpus.push('\n');
        corpus.push_str(&normalize_grounding(&t.name));
    }
    let mut blobs_seen = 0usize;
    for cp in checkpoints {
        let is_getter = spec.entity_tools.iter().any(|g| g == &cp.tool);
        if !is_getter || reporters.contains(&cp.tool.as_str()) {
            for v in cp.args.as_object().into_iter().flatten().map(|(_, v)| v) {
                let Some(s) = v.as_str() else { continue };
                if !s.contains('*') {
                    continue;
                }
                for seg in s.split('*').map(str::trim).filter(|seg| seg.len() >= 3) {
                    if !corpus.contains(&normalize_grounding(seg)) {
                        push(
                            SemanticFindingKind::UngroundedAnswerToken,
                            format!("{tid}: expected {}({}) globs on '{seg}' — checked the prompt, {} tool names, and {blobs_seen} data blob(s) earlier expected calls surface; the word appears in none of them, so the model must guess the grader's wording. Teach it in a blob the intended play reads (or relax the checkpoint)", cp.tool, cp.args, task.tools.len()),
                        );
                    }
                }
            }
        }
        if is_getter {
            let resp = match derive_response(ws_val, &Call { name: cp.tool.clone(), args: cp.args.clone() }) {
                r if r != ACK => r,
                _ => derive_response(ws_val, &Call { name: cp.tool.clone(), args: concretize(&cp.args) }),
            };
            if resp != ACK {
                corpus.push('\n');
                corpus.push_str(&normalize_grounding(&resp));
                blobs_seen += 1;
            }
        }
    }
    out
}

/// Separator-tolerant normalization for grounding lookups: lowercase, with `-`,
/// `_`, `/` all reading as spaces — "mg/kg" grounds "mg kg", "work-product"
/// grounds "work product". Applied identically to corpus and token so the
/// comparison stays consistent.
fn normalize_grounding(s: &str) -> String {
    s.to_lowercase().replace(['-', '_', '/'], " ")
}

/// The oracle's call script for a checkpoint end-state: each checkpoint (concretized), repeated
/// enough times to clear any transient fault on its tool (the fault fires before the advance, so
/// the tool's FIRST occurrence needs `clears_after` extra retries). `None` when the end state is
/// not checkpoint-driven (`RequireEndState` stateful UI / `ExpectAbstainingText`), which this
/// path can't script from a transpiled `ToolTask`.
fn oracle_calls(task: &ToolTask) -> Option<Vec<String>> {
    let spec = task.agentic.as_ref()?;
    let cps = match &spec.end_state {
        EndStateRule::RequireAll(cps) | EndStateRule::RequireSequence(cps) => cps,
        _ => return None,
    };
    let mut calls = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for cp in cps {
        // Transient faults are keyed by tool NAME (a global counter), so only the tool's first
        // appearance needs the extra retries.
        let retries = if seen.insert(cp.tool.clone()) {
            spec.name_faults
                .iter()
                .find(|f| f.on_call == cp.tool)
                .map(|f| match f.fault {
                    FaultInjection::TransientError { clears_after, .. } => clears_after as usize,
                    FaultInjection::PersistentError { .. } => 0,
                })
                .unwrap_or(0)
        } else {
            0
        };
        let body = json!({ "name": cp.tool, "args": concretize(&cp.args) }).to_string();
        for _ in 0..=retries {
            calls.push(body.clone());
        }
    }
    Some(calls)
}

/// Per-task validation verdict, serialized to the UI. `reachable` is `"yes"` / `"no"` /
/// `"not_checkable"` (the last for stateful/abstain end-states this static path can't script —
/// the user must confirm those with a real run). `discriminating` is whether a do-nothing agent
/// correctly FAILS the task (`None` when not applicable). `detail` is the human explanation.
/// `semantic` carries the Error-severity world-state authoring-contract findings
/// (`semantic_findings`) — the same defects `evals::save` hard-blocks on — and fails the
/// collection verdict. `semantic_warnings` carries the Warning-severity heuristics (answer
/// grounding): shown with their evidence so the author can judge, but they neither fail
/// `ok` nor block a save/import — a heuristic that cried wolf would teach authors to
/// ignore it.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct TaskValidation {
    pub id: String,
    pub reachable: String,
    pub discriminating: Option<bool>,
    pub detail: String,
    pub semantic: Vec<String>,
    pub semantic_warnings: Vec<String>,
}

/// Whole-collection verdict. `ok` is true only when there is no structural error AND no task
/// is definitively broken (unreachable or non-discriminating). `structural_error` is `Some`
/// when the schema trust-boundary (`validate_tasks`) rejected the collection outright — then
/// `tasks` is empty (there was nothing well-formed to oracle-check).
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct CollectionValidation {
    pub ok: bool,
    pub structural_error: Option<String>,
    pub tasks: Vec<TaskValidation>,
}

/// Oracle-check ONE (already structurally-valid) task: run the perfect replay agent and, for a
/// checkpoint task, a do-nothing agent, and report reachability + whether the task discriminates.
async fn validate_one(task: &ToolTask) -> TaskValidation {
    // Single-turn (non-agentic) tasks are graded by their `expected` call, not by running a
    // loop — structural validation already proved that field is present and consistent.
    if !is_agentic(&task.category) {
        return TaskValidation {
            id: task.id.clone(),
        semantic: Vec::new(),
        semantic_warnings: Vec::new(),
            reachable: "yes".into(),
            discriminating: None,
            detail: "Single-turn task — graded on its expected call; validated structurally.".into(),
        };
    }

    let Some(calls) = oracle_calls(task) else {
        return TaskValidation {
            id: task.id.clone(),
        semantic: Vec::new(),
        semantic_warnings: Vec::new(),
            reachable: "not_checkable".into(),
            discriminating: None,
            detail: "Stateful or abstain end-state — reachability can't be auto-derived; confirm with a real run.".into(),
        };
    };

    let (sandbox, cfg) = match sandbox_for(task) {
        Ok(v) => v,
        Err(e) => {
            return TaskValidation {
                id: task.id.clone(),
        semantic: Vec::new(),
        semantic_warnings: Vec::new(),
                reachable: "no".into(),
                discriminating: None,
                detail: format!("Could not build the task environment: {e}"),
            }
        }
    };

    // Oracle run: the perfect replay must reach the end state with no unknown-tool calls.
    let oracle = Scripted { calls, next: AtomicUsize::new(0) };
    let (tx, _rx) = unbounded_channel();
    let oracle_out = run_once(&oracle, &sandbox, cfg.max_steps, cfg.max_recovery, 0, &tx).await;
    let reached = matches!(&oracle_out, Ok(o) if o.reached_end && o.unknown_tool_calls == 0 && o.failure.is_none());
    if !reached {
        let why = match &oracle_out {
            Ok(o) if o.unknown_tool_calls > 0 => "a checkpoint names a tool the environment doesn't mock".into(),
            Ok(o) => format!("the oracle ended without success ({:?})", o.failure),
            Err(e) => format!("the run errored ({e})"),
        };
        return TaskValidation {
            id: task.id.clone(),
        semantic: Vec::new(),
        semantic_warnings: Vec::new(),
            reachable: "no".into(),
            discriminating: None,
            detail: format!("Answer key not reachable — {why}. Check the checkpoint tool names, args, and wildcards."),
        };
    }

    // Discrimination floor: a do-nothing agent must FAIL, or the task is trivially passable.
    let lazy = Scripted { calls: vec![], next: AtomicUsize::new(0) };
    let (tx2, _rx2) = unbounded_channel();
    let lazy_out = run_once(&lazy, &sandbox, cfg.max_steps, cfg.max_recovery, 0, &tx2).await;
    let discriminates = matches!(&lazy_out, Ok(o) if !o.reached_end);
    TaskValidation {
        id: task.id.clone(),
        semantic: Vec::new(),
        semantic_warnings: Vec::new(),
        reachable: "yes".into(),
        discriminating: Some(discriminates),
        detail: if discriminates {
            "Reachable by the oracle and a do-nothing agent fails it — solvable and discriminating.".into()
        } else {
            "Reachable, BUT a do-nothing agent also passes — the task is trivially satisfiable and measures nothing.".into()
        },
    }
}

/// Deep-validate a collection: the structural trust boundary first, then the per-task oracle
/// proof. `ok` is false if the structure is rejected OR any task is unreachable / trivially
/// passable. Callers (the Tauri command) load the tasks (bundled, saved, or freshly parsed) and
/// hand them here — the one place both import and authoring get the same guarantee.
pub async fn validate_collection_deep(tasks: &[ToolTask]) -> CollectionValidation {
    if let Err(e) = crate::inference::eval::toolcall::tasks::validate_tasks(tasks) {
        return CollectionValidation { ok: false, structural_error: Some(e.to_string()), tasks: Vec::new() };
    }
    let mut out = Vec::with_capacity(tasks.len());
    let mut all_ok = true;
    for t in tasks {
        let mut v = validate_one(t).await;
        for f in task_semantic_findings(t) {
            match f.severity() {
                SemanticSeverity::Error => v.semantic.push(f.message),
                SemanticSeverity::Warning => v.semantic_warnings.push(f.message),
            }
        }
        // Warnings deliberately do NOT fail `ok`: the grounding check is a heuristic
        // the author must judge; only mechanical certainties block.
        if v.reachable == "no" || v.discriminating == Some(false) || !v.semantic.is_empty() {
            all_ok = false;
        }
        out.push(v);
    }
    CollectionValidation { ok: all_ok, structural_error: None, tasks: out }
}

#[cfg(test)]
#[path = "oracle_tests.rs"]
mod tests;
