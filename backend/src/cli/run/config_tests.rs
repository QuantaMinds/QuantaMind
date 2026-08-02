use super::*;

#[test]
fn save_then_load_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = QmConfig {
        costs: None,
        backend: BackendKind::LlamaCpp,
        model: "qwen2.5:3b".into(),
        collection: "easy-coding".into(),
        profile: "general-agent".into(),
        base: None,
    };
    let rel = cfg.save(dir.path()).unwrap();
    // The returned path is relative (never leaks an absolute path — rule 7f).
    assert_eq!(rel.to_str(), Some("qm.json"));
    assert_eq!(QmConfig::load(dir.path()), Some(cfg));
}

#[test]
fn load_is_none_when_absent_or_broken() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(QmConfig::load(dir.path()), None);
    std::fs::write(dir.path().join(CONFIG_FILE), b"{ not json").unwrap();
    assert_eq!(QmConfig::load(dir.path()), None); // a broken config never crashes a run
}
