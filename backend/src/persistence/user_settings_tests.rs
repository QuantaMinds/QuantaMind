use super::*;
use tempfile::tempdir;

#[test]
fn load_missing_returns_default() {
    let dir = tempdir().unwrap();
    assert_eq!(load(&dir.path().join("nope.yaml")).unwrap(), UserSettings::default());
}

#[test]
fn empty_file_loads_as_default() {
    let dir = tempdir().unwrap();
    let p = dir.path().join("empty.yaml");
    std::fs::write(&p, "").unwrap();
    assert_eq!(load(&p).unwrap(), UserSettings::default());
}

#[test]
fn round_trip_preserves_fields() {
    let dir = tempdir().unwrap();
    let p = dir.path().join("u.yaml");
    let s = UserSettings {
        theme: Some("dark".into()),
        first_run_complete: true,
        last_update_check_at: Some("2026-05-27T10:00:00Z".into()),
        models_folder: Some("/models/shared".into()),
        stt_engine_dir: Some("/opt/homebrew/bin".into()),
        vllm_url: Some("http://34.10.20.30:8000".into()),
        vllm_api_key: Some("secret-vllm".into()),
        sglang_url: Some("http://34.10.20.30:30000".into()),
        sglang_api_key: Some("secret-sglang".into()),
    };
    save(&p, &s).unwrap();
    assert_eq!(load(&p).unwrap(), s);
}

#[test]
fn defaults_are_omitted_from_yaml() {
    let dir = tempdir().unwrap();
    let p = dir.path().join("u.yaml");
    save(&p, &UserSettings::default()).unwrap();
    let raw = std::fs::read_to_string(&p).unwrap();
    assert!(!raw.contains("theme"));
    assert!(!raw.contains("first_run_complete"));
    assert!(!raw.contains("last_update_check_at"));
}
