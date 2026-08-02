use super::*;

#[test]
fn weights_bytes_sums_only_gguf_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("a.gguf"), vec![0u8; 100]).expect("write");
    std::fs::write(dir.path().join("b.GGUF"), vec![0u8; 50]).expect("write");
    std::fs::write(dir.path().join("notes.txt"), vec![0u8; 999]).expect("write");
    assert_eq!(weights_bytes(dir.path()), 150, "only .gguf counts, case-insensitively");
}

#[test]
fn weights_bytes_is_zero_for_a_missing_dir() {
    // A user who has installed nothing yet must still get a usable panel.
    assert_eq!(weights_bytes(std::path::Path::new("/nonexistent/quantamind/weights")), 0);
}

#[test]
fn disk_usage_still_reports_free_space_with_no_weights() {
    let dir = tempfile::tempdir().expect("tempdir");
    let usage = disk_usage_for(dir.path());
    assert_eq!(usage.models_bytes, 0);
    assert!(usage.free_bytes > 0, "free space comes from the filesystem, not a server");
}
