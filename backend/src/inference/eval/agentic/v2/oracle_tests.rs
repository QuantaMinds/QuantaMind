use super::*;
use crate::inference::eval::agentic::v2::collection::load_v2_collection;
use crate::inference::eval::agentic::v2::scenarios::v2_json;

/// A real bundled collection is the golden case: structurally valid, every checkpoint task
/// reachable by the oracle, and (for RequireAll/RequireSequence) discriminating. This is the
/// same guarantee the build-time satisfiability test gives — proven through the USER path.
#[tokio::test]
async fn a_bundled_collection_validates_clean() {
    let tasks = load_v2_collection(v2_json("hard-coding").unwrap()).unwrap();
    let report = validate_collection_deep(&tasks).await;
    assert!(report.structural_error.is_none(), "bundled collection is structurally valid");
    assert!(report.ok, "every bundled task must be reachable + discriminating: {:?}", report.tasks);
    for t in &report.tasks {
        // Every hard-coding task is checkpoint-driven, so it must be positively reachable.
        assert_eq!(t.reachable, "yes", "{} should be reachable: {}", t.id, t.detail);
        assert_ne!(t.discriminating, Some(false), "{} must not be trivially passable", t.id);
    }
}

/// A broken answer key — a checkpoint naming a tool that isn't a presented tool — must FAIL
/// validation. The structural trust boundary catches this shape first (a checkpoint tool must be
/// one the task offers), so it surfaces as a `structural_error`; a subtler break that slips past
/// structure would surface as a `reachable: "no"` task. Either way the author is told, which is
/// the whole point — otherwise every model "fails" the task and the author blames the model.
#[tokio::test]
async fn a_broken_answer_key_fails_validation() {
    let mut tasks = load_v2_collection(v2_json("hard-coding").unwrap()).unwrap();
    let spec = tasks[0].agentic.as_mut().unwrap();
    match &mut spec.end_state {
        EndStateRule::RequireAll(cps) | EndStateRule::RequireSequence(cps) => {
            cps[0].tool = "totally_not_a_real_tool".into();
        }
        _ => panic!("fixture task should be checkpoint-driven"),
    }
    let report = validate_collection_deep(&tasks).await;
    assert!(!report.ok, "a broken answer key must fail validation");
    let surfaced = report.structural_error.is_some() || report.tasks.iter().any(|t| t.reachable == "no");
    assert!(surfaced, "the break must be reported (structural error or an unreachable task)");
}

/// A structurally-invalid collection is rejected up front with the schema error, and NO tasks
/// are oracle-run (there was nothing well-formed to check).
#[tokio::test]
async fn a_structural_error_short_circuits_before_the_oracle() {
    let report = validate_collection_deep(&[]).await; // empty ⇒ validate_tasks rejects
    assert!(!report.ok);
    assert!(report.structural_error.is_some(), "empty collection is a structural error");
    assert!(report.tasks.is_empty(), "no per-task oracle results when the structure is rejected");
}

fn ecommerce_tasks() -> Vec<ToolTask> {
    load_v2_collection(v2_json("hard-support-ecommerce").unwrap()).unwrap()
}

fn ws_of(task: &mut ToolTask) -> &mut serde_json::Map<String, Value> {
    task.agentic.as_mut().unwrap().world_state.as_mut().unwrap().as_object_mut().unwrap()
}

/// Every bundled collection is clean under the semantic contract (the CI guards in
/// scenarios.rs sweep all 22; this pins the shared function itself on one).
#[test]
fn semantic_findings_clean_on_a_bundled_collection() {
    let tasks = ecommerce_tasks();
    let findings = semantic_findings(&tasks);
    assert!(findings.is_empty(), "bundled collection must be semantically clean: {findings:?}");
}

/// A digit-bearing entity id named in neither the prompt nor any other blob is an
/// orphan — the model has no path to it.
#[test]
fn semantic_findings_flags_an_orphan_entity() {
    let mut tasks = ecommerce_tasks();
    ws_of(&mut tasks[0]).insert("Z-99".into(), serde_json::json!({ "status": "lost" }));
    let findings = semantic_findings(&tasks);
    let orphan = findings.iter().find(|f| f.kind == SemanticFindingKind::OrphanEntity);
    let orphan = orphan.expect("an unreferenced Z-99 entity must be flagged as orphaned");
    assert_eq!(orphan.task_id, tasks[0].id);
    assert!(orphan.message.contains("Z-99"), "finding names the key: {}", orphan.message);
}

/// Deleting the entity an expected getter fetches turns that getter into an ack —
/// the exact hard-support-ecommerce bug class (fact parked out of reach).
#[test]
fn semantic_findings_flags_an_acking_expected_getter() {
    let mut tasks = ecommerce_tasks();
    ws_of(&mut tasks[0]).remove("MShop").expect("fixture has the MShop policy entity");
    let findings = semantic_findings(&tasks);
    let acking = findings.iter().find(|f| f.kind == SemanticFindingKind::AckingGetter);
    let acking = acking.expect("an expected getter resolving to no data must be flagged");
    assert_eq!(acking.task_id, tasks[0].id);
    assert!(acking.message.contains("MShop"), "finding names the unresolvable arg: {}", acking.message);
}

/// A digit-free, non-reserved key no intended path reaches is leakable oracle data.
#[test]
fn semantic_findings_flags_an_unfetched_oracle_key() {
    let mut tasks = ecommerce_tasks();
    ws_of(&mut tasks[0]).insert("secret_answers".into(), serde_json::json!({ "O-1": "FULL_REFUND" }));
    let findings = semantic_findings(&tasks);
    let leak = findings.iter().find(|f| f.kind == SemanticFindingKind::UnfetchedKey);
    let leak = leak.expect("an unfetched oracle key must be flagged as leakable");
    assert!(leak.message.contains("secret_answers"), "finding names the key: {}", leak.message);
}

/// Non-entity environments and non-agentic tasks are out of the contract's scope.
#[test]
fn semantic_findings_skips_fs_env_and_single_turn_tasks() {
    let fs_tasks = load_v2_collection(v2_json("easy-coding-fs").unwrap()).unwrap();
    assert!(semantic_findings(&fs_tasks).is_empty(), "filesystem env has no entity responder");

    let single = ToolTask {
        id: "s".into(),
        category: "single".into(),
        prompt: "p".into(),
        tools: vec![],
        expected: Default::default(),
        agentic: None,
    };
    assert!(semantic_findings(&[single]).is_empty(), "single-turn tasks have no world_state contract");
}

/// The deep validator carries the semantic findings into the per-task verdict and
/// fails the collection — this is the payload the import dry-run popup renders.
#[tokio::test]
async fn validate_collection_deep_reports_semantic_findings() {
    let mut tasks = ecommerce_tasks();
    ws_of(&mut tasks[0]).insert("Z-99".into(), serde_json::json!({ "status": "lost" }));
    let report = validate_collection_deep(&tasks).await;
    assert!(!report.ok, "semantic findings must fail the collection verdict");
    let t0 = report.tasks.iter().find(|t| t.id == tasks[0].id).unwrap();
    assert!(t0.semantic.iter().any(|m| m.contains("Z-99")), "verdict carries the finding text: {:?}", t0.semantic);
}

/// Point an expected action checkpoint at a word NOTHING teaches — the grounding
/// check must flag it, as a WARNING (it is a heuristic: the audit that motivated
/// it produced false positives needing human triage).
fn with_ungrounded_token(tasks: &mut [ToolTask]) {
    let spec = tasks[0].agentic.as_mut().unwrap();
    let EndStateRule::RequireAll(cps) = &mut spec.end_state else { panic!("fixture is RequireAll") };
    let cp = cps.iter_mut().find(|c| c.tool == "log_decision").expect("fixture logs decisions");
    cp.args["decision"] = serde_json::json!("*xylophone_zebra*");
}

#[test]
fn semantic_findings_flags_an_ungrounded_answer_token_as_a_warning() {
    let mut tasks = ecommerce_tasks();
    with_ungrounded_token(&mut tasks);
    let findings = semantic_findings(&tasks);
    let f = findings.iter().find(|f| f.kind == SemanticFindingKind::UngroundedAnswerToken);
    let f = f.expect("an answer word nothing teaches must be flagged");
    assert_eq!(f.severity(), SemanticSeverity::Warning, "grounding is a heuristic — never a hard error");
    assert!(f.message.contains("xylophone_zebra"), "names the token: {}", f.message);
    assert!(f.message.contains("checked the prompt"), "carries the evidence of where it looked: {}", f.message);
}

/// Separator variants ARE grounding: `medium-legal` grades on "work product" /
/// "legal hold" which its blobs teach as "work-product" / `legal_hold` — the
/// normalized corpus must accept them (this bundled collection is the regression
/// fixture; flagging it would be exactly the cried-wolf false positive that
/// teaches authors to ignore the warning).
#[test]
fn grounding_accepts_separator_variant_wording() {
    let tasks = load_v2_collection(v2_json("medium-legal").unwrap()).unwrap();
    let ungrounded: Vec<_> = semantic_findings(&tasks)
        .into_iter()
        .filter(|f| f.kind == SemanticFindingKind::UngroundedAnswerToken)
        .collect();
    assert!(ungrounded.is_empty(), "separator-variant wording is grounded: {ungrounded:?}");
}

/// Warnings surface in `semantic_warnings` with the evidence, but do NOT fail the
/// collection verdict — the author judges a heuristic; only certainties block.
#[tokio::test]
async fn grounding_warnings_do_not_fail_the_deep_verdict() {
    let mut tasks = ecommerce_tasks();
    with_ungrounded_token(&mut tasks);
    let report = validate_collection_deep(&tasks).await;
    let t0 = report.tasks.iter().find(|t| t.id == tasks[0].id).unwrap();
    assert!(t0.semantic_warnings.iter().any(|m| m.contains("xylophone_zebra")), "warning surfaced: {:?}", t0.semantic_warnings);
    assert!(t0.semantic.is_empty(), "a warning is not an error");
    assert!(report.ok, "warnings alone must not fail the collection");
}
