use super::*;
use crate::inference::eval::toolcall::eval::TaskTrace;
use crate::inference::eval::toolcall::score::Verdict;
use tempfile::tempdir;

fn trace(id: &str, raw: &str) -> TaskTrace {
    TaskTrace {
        id: id.into(),
        category: "single".into(),
        trace: TraceResult {
            system_message: "sys".into(),
            user_prompt: "ask".into(),
            raw_output: raw.into(),
            verdict: Verdict { parsed: true, tool_match: true, args_match: true, abstain_correct: None },
            prompt_tokens: Some(42),
        },
    }
}

#[test]
fn load_one_missing_is_none() {
    let dir = tempdir().unwrap();
    assert!(load_one(dir.path(), "mine", "llama", "t1").unwrap().is_none());
}

#[test]
fn upsert_then_load_one_round_trips() {
    let dir = tempdir().unwrap();
    upsert(dir.path(), "mine", "llama", BackendKind::LlamaCpp, &[trace("t1", "out-1")]).unwrap();
    let got = load_one(dir.path(), "mine", "llama", "t1").unwrap().unwrap();
    assert_eq!(got.raw_output, "out-1");
    assert!(got.verdict.tool_match);
    // A different model / task isn't matched.
    assert!(load_one(dir.path(), "mine", "other", "t1").unwrap().is_none());
    assert!(load_one(dir.path(), "mine", "llama", "t2").unwrap().is_none());
}

#[test]
fn incremental_upserts_accumulate_and_replace_by_task_id() {
    let dir = tempdir().unwrap();
    // Simulator streams one task per call — both must survive.
    upsert(dir.path(), "mine", "llama", BackendKind::LlamaCpp, &[trace("t1", "first")]).unwrap();
    upsert(dir.path(), "mine", "llama", BackendKind::LlamaCpp, &[trace("t2", "second")]).unwrap();
    assert_eq!(load_one(dir.path(), "mine", "llama", "t1").unwrap().unwrap().raw_output, "first");
    assert_eq!(load_one(dir.path(), "mine", "llama", "t2").unwrap().unwrap().raw_output, "second");
    // Re-running t1 replaces its trace, doesn't duplicate.
    upsert(dir.path(), "mine", "llama", BackendKind::LlamaCpp, &[trace("t1", "fresh")]).unwrap();
    assert_eq!(load_one(dir.path(), "mine", "llama", "t1").unwrap().unwrap().raw_output, "fresh");
}

#[test]
fn many_models_coexist_in_one_collection_file() {
    let dir = tempdir().unwrap();
    upsert(dir.path(), "mine", "llama", BackendKind::LlamaCpp, &[trace("t1", "a")]).unwrap();
    upsert(dir.path(), "mine", "qwen", BackendKind::VLlm, &[trace("t1", "b")]).unwrap();
    assert_eq!(load_one(dir.path(), "mine", "llama", "t1").unwrap().unwrap().raw_output, "a");
    assert_eq!(load_one(dir.path(), "mine", "qwen", "t1").unwrap().unwrap().raw_output, "b");
}

#[test]
fn oversize_file_is_guarded() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path()).unwrap();
    std::fs::write(dir.path().join("mine.json"), vec![b' '; (MAX_BYTES + 1) as usize]).unwrap();
    assert!(load_one(dir.path(), "mine", "llama", "t1").is_err());
}

#[test]
fn bad_collection_id_rejected() {
    let dir = tempdir().unwrap();
    for id in ["../escape", "a/b", "", "..", ".hidden"] {
        assert!(upsert(dir.path(), id, "llama", BackendKind::LlamaCpp, &[trace("t", "x")]).is_err(), "should reject {id:?}");
        assert!(load_one(dir.path(), id, "llama", "t").is_err());
    }
}
