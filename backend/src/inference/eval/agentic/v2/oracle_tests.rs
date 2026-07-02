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
