use super::*;
use std::fs;
use tempfile::tempdir;

fn collapsed(d: u32) -> CliffStatus {
    CliffStatus::Collapsed { depth: d, concentration: None }
}

#[test]
fn round_trips_a_colon_bearing_model_verbatim() {
    let dir = tempdir().unwrap();
    save(dir.path(), "finance", "qwen2.5-coder:7b", collapsed(8192)).unwrap();
    let map = load(dir.path(), "finance").unwrap();
    assert_eq!(map.get("qwen2.5-coder:7b"), Some(&collapsed(8192))); // colon key preserved
}

#[test]
fn no_cliff_status_round_trips() {
    let dir = tempdir().unwrap();
    save(dir.path(), "c", "m", CliffStatus::NoCliff { tested: 4000 }).unwrap();
    assert_eq!(load(dir.path(), "c").unwrap().get("m"), Some(&CliffStatus::NoCliff { tested: 4000 }));
}

#[test]
fn broken_status_round_trips() {
    let dir = tempdir().unwrap();
    save(dir.path(), "c", "m", CliffStatus::Broken { tested: 388 }).unwrap();
    assert_eq!(load(dir.path(), "c").unwrap().get("m"), Some(&CliffStatus::Broken { tested: 388 }));
}

#[test]
fn second_save_merges_and_does_not_clobber_other_models() {
    let dir = tempdir().unwrap();
    save(dir.path(), "finance", "a:1", collapsed(1000)).unwrap();
    save(dir.path(), "finance", "b:2", CliffStatus::NoCliff { tested: 2000 }).unwrap();
    let map = load(dir.path(), "finance").unwrap();
    assert_eq!(map.get("a:1"), Some(&collapsed(1000))); // first model survives the second save
    assert_eq!(map.get("b:2"), Some(&CliffStatus::NoCliff { tested: 2000 }));

    // Last-write-wins per model.
    save(dir.path(), "finance", "a:1", collapsed(1500)).unwrap();
    assert_eq!(load(dir.path(), "finance").unwrap().get("a:1"), Some(&collapsed(1500)));
}

#[test]
fn migrates_legacy_bare_u32_entries_to_collapsed() {
    // The old store wrote `{ "m": 4000 }` — a bare collapse depth (at the same
    // safe_filename-hashed path the new store uses).
    let dir = tempdir().unwrap();
    let path = dir.path().join(format!("{}.json", safe_filename("finance")));
    fs::write(&path, r#"{ "qwen:7b": 4000 }"#).unwrap();
    let map = load(dir.path(), "finance").unwrap();
    assert_eq!(map.get("qwen:7b"), Some(&collapsed(4000)));
}

#[test]
fn save_is_atomic_leaves_no_temp_file_and_stays_parseable() {
    let dir = tempdir().unwrap();
    save(dir.path(), "c", "m", collapsed(4096)).unwrap();
    let names: Vec<String> =
        fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    assert!(names.iter().all(|n| !n.ends_with(".tmp")), "no temp file should remain: {names:?}");
    assert!(load(dir.path(), "c").is_ok()); // the live file always parses
}

#[test]
fn missing_collection_loads_an_empty_map() {
    let dir = tempdir().unwrap();
    assert!(load(dir.path(), "never").unwrap().is_empty());
}
