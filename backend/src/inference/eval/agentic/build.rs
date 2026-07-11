use crate::errors::{AppError, AppResult};
use crate::inference::eval::agentic::difficulty::decoys;
use crate::inference::eval::agentic::difficulty::passk::{max_steps_for, pass_k_for};
use crate::inference::eval::agentic::runner::AgenticConfig;
use crate::inference::eval::agentic::sandbox::{DeterministicSandbox, EndStateRule};
use crate::inference::eval::agentic::spec::{EnvKind, SafetyArm};
use crate::inference::eval::agentic::v2::env_corpus::CorpusState;
use crate::inference::eval::agentic::v2::env_fs::FsState;
use crate::inference::eval::agentic::v2::env_webui::WebUiSpec;
use crate::inference::eval::toolcall::tasks::ToolTask;

/// FNV-1a over the task id: a stable, dependency-free seed so a task's decoy set is
/// reproducible across machines/runs yet differs per task. Decoys are fixed for the
/// whole Pass^k batch (one sandbox per task), so this is keyed on the task, not the
/// run index. The per-instance generator seed (Phase 9C) is a separate concern.
fn seed_from_id(id: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Project an agentic `ToolTask` into a ready-to-run sandbox + config. Defense in
/// depth beyond `validate_tasks`: confirms the task is agentic and that every
/// checkpoint / mock names a declared tool. The task's `prompt` becomes the
/// initial prompt; an absent `k`/`max_steps` falls back to the tier policy
/// (`pass_k_for` / `max_steps_for`), so a harder spec gets a larger budget by default.
pub fn sandbox_for(task: &ToolTask) -> AppResult<(DeterministicSandbox, AgenticConfig)> {
    let spec = task
        .agentic
        .as_ref()
        .ok_or_else(|| AppError::InvalidTaskSchema(format!("task '{}' is not agentic", task.id)))?;
    let known = |name: &str| task.tools.iter().any(|t| t.name == name);
    let checkpoints: &[_] = match &spec.end_state {
        EndStateRule::RequireSequence(cps) | EndStateRule::RequireAll(cps) => cps,
        EndStateRule::ExpectAbstainingText | EndStateRule::RequireEndState(_) => &[],
    };
    for cp in checkpoints {
        if !known(&cp.tool) {
            return Err(AppError::InvalidTaskSchema(format!(
                "task '{}' end-state checkpoint calls unknown tool '{}'",
                task.id, cp.tool
            )));
        }
    }
    for m in &spec.mocks {
        if !known(&m.call.name) {
            return Err(AppError::InvalidTaskSchema(format!(
                "task '{}' mock references unknown tool '{}'",
                task.id, m.call.name
            )));
        }
    }
    for f in &spec.faults {
        if !known(&f.call.name) {
            return Err(AppError::InvalidTaskSchema(format!(
                "task '{}' fault references unknown tool '{}'",
                task.id, f.call.name
            )));
        }
    }
    // Phase 9A: v1 ("agentic") tasks get `axes.decoy_tools` random-pool distractors
    // shuffled in (deterministic per task). v2 ("agent_loop") tasks carry their OWN
    // authored decoy_tools already merged into `task.tools`, so they skip the pool.
    let decoy_n = if task.category == "agentic" { spec.axes.as_ref().map(|a| a.decoy_tools).unwrap_or(0) } else { 0 };
    let presented = decoys::merge_decoys(&task.tools, decoy_n, seed_from_id(&task.id));
    let mut sandbox = DeterministicSandbox::new(
        task.prompt.clone(),
        presented,
        spec.mocks.clone(),
        spec.end_state.clone(),
    )
    .with_faults(spec.faults.clone())
    .with_must_not_call(spec.must_not_call.clone());
    // v2: ground-truth responder when the task carries a world_state. `Filesystem` builds the
    // simulated-filesystem responder (path→content; getters return real content); the default
    // `Entity` keeps the world_state responder, where the getter set (authored
    // `returns_entity`) splits entity-returning tools from acting tools.
    if let Some(ws) = &spec.world_state {
        sandbox = match spec.environment {
            EnvKind::Filesystem => sandbox.with_filesystem(FsState::from_world_state(ws)),
            EnvKind::WebCorpus => sandbox.with_web_corpus(CorpusState::from_world_state(ws)),
            EnvKind::WebUi => sandbox.with_web_ui(WebUiSpec::from_world_state(ws)),
            EnvKind::Entity => sandbox.with_world_state(ws.clone()).with_entity_tools(spec.entity_tools.clone()),
        };
    }
    // Recognized real-tool whitelist (getters + actions) so a decoy/hallucinated call in
    // WorldState mode gets the corrective nudge, not a misleading `{"ok":true}`. v1
    // ("agentic"): `task.tools` is still decoy-free here (pool decoys went into
    // `presented` above), so derive from it. v2 ("agent_loop"): `task.tools` already
    // carries the authored decoys (merged in `transpile`), so use the pre-merge names the
    // transpiler stashed in `spec.recognized_tools`.
    let recognized: Vec<String> = if task.category == "agentic" {
        task.tools.iter().map(|t| t.name.clone()).collect()
    } else {
        spec.recognized_tools.clone()
    };
    sandbox = sandbox.with_recognized_tools(recognized);
    // Category K: attach the standing-guard marker for an Attack-arm safety probe, so a
    // forbidden-call terminus is attributable (model followed the injection vs the config's
    // window evicted the guard). A benign-control probe carries no trap to attribute, so it
    // never sets a guard.
    if let Some(safety) = &spec.safety {
        if safety.arm == SafetyArm::Attack {
            sandbox = sandbox.with_safety_guard(safety.guard.marker.clone());
        }
    }
    // Payload noise: wrap world_state getter blobs in messy JSON so the model must extract
    // the right field from noise.
    if spec.payload_noise {
        sandbox = sandbox.with_payload_noise(true);
    }
    // v2: name-keyed faults (on_call trips on any call to that tool).
    if !spec.name_faults.is_empty() {
        let nf: std::collections::HashMap<String, crate::inference::eval::agentic::spec::FaultInjection> =
            spec.name_faults.iter().map(|f| (f.on_call.clone(), f.fault.clone())).collect();
        sandbox = sandbox.with_name_faults(nf);
    }
    let d = AgenticConfig::default();
    let cfg = AgenticConfig {
        // Precedence (Gap 4): an explicit `k` (authored, or the UI K override that
        // `apply_overrides` writes into `spec.k`) wins; otherwise scale by tier.
        k: spec.k.unwrap_or_else(|| pass_k_for(spec.tier)),
        // Same precedence for the step budget: an explicit `max_steps` (authored or the
        // UI Max-Steps field) wins; otherwise scale the horizon by tier (`d.max_steps` is
        // no longer a flat floor — an untiered/Easy spec resolves to `max_steps_for(Easy)`).
        max_steps: spec.max_steps.unwrap_or_else(|| max_steps_for(spec.tier)),
        max_recovery: spec.max_recovery.unwrap_or(d.max_recovery),
    };
    Ok((sandbox, cfg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::eval::agentic::sandbox::{MockResponse, TaskCheckpoint};
    use crate::inference::eval::agentic::spec::{AgenticSpec, DifficultyAxes, Tier};
    use crate::inference::eval::toolcall::tasks::{Call, ToolSchema};
    use serde_json::json;

    fn tool(name: &str) -> ToolSchema {
        ToolSchema {
            name: name.into(),
            description: "d".into(),
            parameters: json!({ "type": "object", "properties": {} }),
        }
    }

    fn agentic_task() -> ToolTask {
        ToolTask {
            id: "fin".into(),
            category: "agentic".into(),
            prompt: "Transfer the balance.".into(),
            tools: vec![tool("get_balance"), tool("transfer")],
            expected: Default::default(),
            agentic: Some(AgenticSpec {
                mocks: vec![MockResponse {
                    call: Call { name: "get_balance".into(), args: json!({ "id": "A" }) },
                    response: "{}".into(),
                }],
                end_state: EndStateRule::RequireSequence(vec![TaskCheckpoint {
                    tool: "transfer".into(),
                    args: json!({ "amount": 1.0 }),
                }]),
                environment: Default::default(),
                tier: Default::default(),
                axes: None,
                k: None,
                max_steps: Some(7),
                faults: vec![],
                max_recovery: None,
                must_not_call: vec![],
                world_state: None,
                name_faults: vec![],
                generated: false,
                entity_tools: vec![],
                recognized_tools: vec![],
                safety: None,
                payload_noise: false,
            }),
        }
    }

    #[test]
    fn builds_sandbox_and_applies_config_defaults() {
        let (sandbox, cfg) = sandbox_for(&agentic_task()).unwrap();
        assert_eq!(sandbox.initial_prompt, "Transfer the balance.");
        assert!(sandbox.respond(&Call { name: "get_balance".into(), args: json!({ "id": "A" }) }).is_some());
        assert_eq!(cfg.k, 5); // default
        assert_eq!(cfg.max_steps, 7); // override
    }

    #[test]
    fn decoy_axis_injects_distractors_that_cannot_satisfy_the_end_state() {
        let mut t = agentic_task();
        t.id = "decoy-task".into();
        t.agentic.as_mut().unwrap().axes =
            Some(DifficultyAxes { decoy_tools: 4, ..Default::default() });
        let (sandbox, _) = sandbox_for(&t).unwrap();

        // The real (mockable) tools survive...
        assert!(sandbox.tools.iter().any(|x| x.name == "get_balance"));
        assert!(sandbox.tools.iter().any(|x| x.name == "transfer"));
        // ...plus exactly 4 decoys.
        assert_eq!(sandbox.tools.len(), 2 + 4);
        // A decoy is schema-valid but has no mock, so respond() is None — calling it
        // yields the runner's "unknown tool" injection, never an end-state advance.
        let decoy =
            sandbox.tools.iter().find(|x| x.name != "get_balance" && x.name != "transfer").unwrap();
        assert!(sandbox.respond(&Call { name: decoy.name.clone(), args: json!({}) }).is_none());
    }

    #[test]
    fn tier_scales_k_when_no_explicit_k_is_authored() {
        let mut t = agentic_task();
        t.agentic.as_mut().unwrap().k = None; // no explicit k → tier policy decides
        t.agentic.as_mut().unwrap().tier = Tier::Hard;
        let (_, cfg) = sandbox_for(&t).unwrap();
        assert_eq!(cfg.k, 16); // pass_k_for(Hard)
    }

    #[test]
    fn an_explicit_k_wins_over_the_tier_policy() {
        let mut t = agentic_task();
        t.agentic.as_mut().unwrap().k = Some(3); // authored / UI override
        t.agentic.as_mut().unwrap().tier = Tier::Extreme; // would be 24
        let (_, cfg) = sandbox_for(&t).unwrap();
        assert_eq!(cfg.k, 3); // explicit k beats pass_k_for(Extreme)
    }

    #[test]
    fn tier_scales_max_steps_when_no_explicit_value_is_authored() {
        let mut t = agentic_task();
        t.agentic.as_mut().unwrap().max_steps = None; // no explicit cap → tier policy decides
        t.agentic.as_mut().unwrap().tier = Tier::Hard;
        let (_, cfg) = sandbox_for(&t).unwrap();
        assert_eq!(cfg.max_steps, 32); // max_steps_for(Hard), not the old flat default
    }

    #[test]
    fn an_explicit_max_steps_wins_over_the_tier_policy() {
        let mut t = agentic_task();
        t.agentic.as_mut().unwrap().max_steps = Some(5); // authored / UI override
        t.agentic.as_mut().unwrap().tier = Tier::Extreme; // would be 48
        let (_, cfg) = sandbox_for(&t).unwrap();
        assert_eq!(cfg.max_steps, 5); // explicit cap beats max_steps_for(Extreme)
    }

    #[test]
    fn the_decoy_set_is_deterministic_for_a_given_task() {
        let mut t = agentic_task();
        t.id = "stable-id".into();
        t.agentic.as_mut().unwrap().axes =
            Some(DifficultyAxes { decoy_tools: 5, ..Default::default() });
        let names = |t: &ToolTask| -> Vec<String> {
            sandbox_for(t).unwrap().0.tools.iter().map(|x| x.name.clone()).collect()
        };
        assert_eq!(names(&t), names(&t)); // same task → identical presented order
    }

    #[test]
    fn rejects_a_non_agentic_task() {
        let mut t = agentic_task();
        t.agentic = None;
        assert!(sandbox_for(&t).is_err());
    }

    #[test]
    fn v2_decoy_yields_unknown_tool_not_a_misleading_ack() {
        // A v2 ("agent_loop") task carries its decoys already merged into `task.tools`,
        // and the pre-merge real names in `spec.recognized_tools`. sandbox_for must build
        // the whitelist from the latter so a decoy call returns None (→ runner nudge),
        // while the recognized getter/action still resolve. This is the regression for the
        // misleading `{"ok":true}` decoy bug.
        let mut t = agentic_task();
        t.id = "v2-decoy".into();
        t.category = "agent_loop".into(); // v2: skips the decoy pool; uses recognized_tools
        t.tools = vec![tool("get_dep"), tool("pin_and_flag"), tool("read_file")]; // read_file is the decoy
        let spec = t.agentic.as_mut().unwrap();
        spec.world_state = Some(json!({ "D-1": { "kind": "major" } }));
        spec.entity_tools = vec!["get_dep".into()];
        spec.recognized_tools = vec!["get_dep".into(), "pin_and_flag".into()]; // decoy excluded
        spec.end_state = EndStateRule::RequireAll(vec![TaskCheckpoint { tool: "pin_and_flag".into(), args: json!({}) }]);
        spec.mocks = vec![]; // v2 uses the world_state responder, not static mocks

        let (sandbox, _) = sandbox_for(&t).unwrap();
        // The decoy is present in the presented tool list (the model can see/call it)...
        assert!(sandbox.tools.iter().any(|x| x.name == "read_file"));
        // ...but calling it now nudges instead of acking, while real tools resolve.
        assert!(sandbox.respond(&Call { name: "read_file".into(), args: json!({ "path": "x" }) }).is_none());
        assert_eq!(
            sandbox.respond(&Call { name: "get_dep".into(), args: json!({ "id": "D-1" }) }).as_deref(),
            Some(r#"{"kind":"major"}"#)
        );
        assert_eq!(
            sandbox.respond(&Call { name: "pin_and_flag".into(), args: json!({ "dep": "D-1" }) }).as_deref(),
            Some(r#"{"ok":true}"#)
        );
    }
}
