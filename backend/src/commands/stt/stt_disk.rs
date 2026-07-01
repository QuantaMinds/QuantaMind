use crate::commands::storage::storage_disk::absolutize;
use std::path::{Path, PathBuf};

/// App-owned STT models folder — the source of truth for installed whisper.cpp
/// models and the shared silero VAD, kept out of the scattered per-tool caches.
/// Precedence: user setting → `QUANTAMIND_STT_DIR` env → `~/.quantamind/stt`.
/// Mirrors `storage_disk::gguf_dir_resolved`.
pub fn stt_dir_resolved(setting: Option<&str>) -> PathBuf {
    if let Some(p) = setting.filter(|s| !s.trim().is_empty()) {
        return absolutize(PathBuf::from(p));
    }
    if let Ok(p) = std::env::var("QUANTAMIND_STT_DIR") {
        return absolutize(PathBuf::from(p));
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    PathBuf::from(home).join(".quantamind/stt")
}

/// The default/env-resolved STT folder (no user-setting override).
pub fn stt_dir() -> PathBuf {
    stt_dir_resolved(None)
}

/// Canonical path for a whisper ggml model identified by `name` (a catalog id
/// like `tiny.en`), sanitizing `:`/`/` and prefixing `ggml-` so the on-disk
/// file matches the repo's `ggml-tiny.en.bin` naming.
pub fn whisper_dest(dir: &Path, name: &str) -> PathBuf {
    let safe = name.replace([':', '/'], "_");
    dir.join(format!("ggml-{safe}.bin"))
}

/// Canonical path for the silero VAD file `name` (already a repo filename such
/// as `ggml-silero-v5.1.2.bin`), sanitizing `:`/`/` for safety.
pub fn vad_dest(dir: &Path, name: &str) -> PathBuf {
    let safe = name.replace([':', '/'], "_");
    dir.join(safe)
}

/// The root holding per-install staging dirs.
pub fn staging_root(dir: &Path) -> PathBuf {
    dir.join(".staging")
}

/// Per-install staging dir. A model's files download here and are promoted
/// (renamed) into place only once *all* validate, so a crash mid-install never
/// leaves a half-installed pair (R3).
pub fn staging_dir(dir: &Path, id: &str) -> PathBuf {
    staging_root(dir).join(id.replace([':', '/'], "_"))
}

/// Sweep orphaned install artifacts: the whole `.staging` tree and any stray
/// `.partial` resume markers. Run at app init and before each download so a
/// model is reported installed only when its real files are present — never
/// half-installed. Returns the count of entries removed.
pub fn reconcile_stt_dir(dir: &Path) -> std::io::Result<u64> {
    if !dir.exists() {
        return Ok(0);
    }
    let mut removed = 0;
    let staging = staging_root(dir);
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
        removed += 1;
    }
    for entry in std::fs::read_dir(dir)? {
        let p = entry?.path();
        if p.is_file() && p.extension().is_some_and(|e| e == "partial") {
            std::fs::remove_file(&p)?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whisper_dest_prefixes_ggml_and_sanitizes_the_model_id() {
        let p = whisper_dest(Path::new("/s"), "tiny.en");
        assert_eq!(p, PathBuf::from("/s/ggml-tiny.en.bin"));
        let p = whisper_dest(Path::new("/s"), "large-v3");
        assert_eq!(p, PathBuf::from("/s/ggml-large-v3.bin"));
    }

    #[test]
    fn whisper_dest_replaces_path_separators_from_exotic_ids() {
        let p = whisper_dest(Path::new("/s"), "vendor/tiny:q5");
        assert_eq!(p, PathBuf::from("/s/ggml-vendor_tiny_q5.bin"));
    }

    #[test]
    fn vad_dest_keeps_the_repo_filename_sanitized() {
        let p = vad_dest(Path::new("/s"), "ggml-silero-v5.1.2.bin");
        assert_eq!(p, PathBuf::from("/s/ggml-silero-v5.1.2.bin"));
    }

    // One test owns QUANTAMIND_STT_DIR — cargo runs tests in parallel, so a
    // second env-mutating test would race this one. Uses `std::env::temp_dir`
    // for a real cross-platform absolute path — `/tmp/...` isn't absolute on
    // Windows, so `absolutize()` prefixes it with the drive letter and the
    // equality assertion fails (identical bug pattern to the Phase 4 fix in
    // `storage_disk_tests.rs`).
    #[test]
    fn stt_dir_precedence_setting_then_env_then_default() {
        let env_path = std::env::temp_dir().join("qm-stt-test");
        let setting_path = std::env::temp_dir().join("qm-models-stt");
        std::env::set_var("QUANTAMIND_STT_DIR", &env_path);
        assert_eq!(stt_dir(), env_path, "env beats default");
        assert_eq!(
            stt_dir_resolved(Some(setting_path.to_str().unwrap())),
            setting_path,
            "setting beats env"
        );
        assert_eq!(
            stt_dir_resolved(Some("  ")),
            env_path,
            "blank setting falls through to env"
        );
        std::env::remove_var("QUANTAMIND_STT_DIR");
        let default = stt_dir_resolved(None);
        let leaf = default.file_name().and_then(|s| s.to_str()).unwrap_or("");
        assert_eq!(leaf, "stt", "default falls through to <data-dir>/stt");
    }

    #[test]
    fn relative_setting_resolves_to_an_absolute_path() {
        let resolved = stt_dir_resolved(Some("./stt"));
        assert!(resolved.is_absolute(), "expected absolute, got {resolved:?}");
        assert!(resolved.ends_with("stt"));
    }

    #[test]
    fn staging_dir_is_under_dot_staging_and_sanitized() {
        let s = staging_dir(Path::new("/s"), "large-v3");
        assert_eq!(s, PathBuf::from("/s/.staging/large-v3"));
        assert_eq!(staging_dir(Path::new("/s"), "a/b:c"), PathBuf::from("/s/.staging/a_b_c"));
    }

    #[test]
    fn reconcile_removes_staging_and_partials_keeps_real_models() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::create_dir_all(staging_dir(p, "tiny.en")).unwrap();
        std::fs::write(staging_dir(p, "tiny.en").join("ggml-tiny.en.bin.partial"), b"x").unwrap();
        std::fs::write(p.join("ggml-base.en.bin.partial"), b"x").unwrap();
        std::fs::write(p.join("ggml-tiny.en.bin"), b"real-model").unwrap();

        let removed = reconcile_stt_dir(p).unwrap();
        assert!(removed >= 2, "removed staging tree + stray partial");
        assert!(!staging_root(p).exists(), "staging swept");
        assert!(!p.join("ggml-base.en.bin.partial").exists(), "stray partial swept");
        assert!(p.join("ggml-tiny.en.bin").exists(), "a real model file is kept");
    }

    #[test]
    fn reconcile_on_a_missing_dir_is_a_noop() {
        assert_eq!(reconcile_stt_dir(Path::new("/no/such/qm-stt-dir")).unwrap(), 0);
    }
}
