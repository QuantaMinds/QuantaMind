use quantamind_lib::commands::storage::storage_disk::compute_disk_usage;
use std::path::Path;

#[test]
fn disk_usage_carries_models_bytes_through_unchanged() {
    // Even when total/free can't be determined, the caller-supplied
    // models_bytes sum must round-trip exactly.
    let usage = compute_disk_usage(Path::new("/__nonexistent_zzz/test"), 12345);
    assert_eq!(usage.models_bytes, 12345);
}

#[test]
fn disk_usage_for_a_real_path_reports_nonzero_total_and_sensible_free() {
    let cwd = std::env::current_dir().expect("cwd should be available");
    let usage = compute_disk_usage(&cwd, 0);
    assert!(usage.total_bytes > 0, "a real path must report a total");
    assert!(usage.free_bytes <= usage.total_bytes, "free can never exceed total");
}
