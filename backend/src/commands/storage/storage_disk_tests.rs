use super::*;
use std::sync::Mutex;

// `QUANTAMIND_GGUF_DIR` / `QUANTAMIND_MLX_DIR` are process-global state, but
// `cargo test` runs tests in parallel threads within the same process. Every
// test below that reads or mutates one of these vars holds the matching lock
// for its full body so the two groups can't interleave and observe each
// other's env-var writes.
static GGUF_ENV_LOCK: Mutex<()> = Mutex::new(());
static MLX_ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn gguf_dest_sanitizes_model_tag_into_a_filename() {
    let p = gguf_dest(Path::new("/g"), "llama3.2:1b");
    assert_eq!(p, PathBuf::from("/g/llama3.2_1b.gguf"));
}

#[test]
fn gguf_dest_replaces_slashes_from_repo_style_names() {
    let p = gguf_dest(Path::new("/g"), "meta/llama:8b");
    assert_eq!(p, PathBuf::from("/g/meta_llama_8b.gguf"));
}

// Holds GGUF_ENV_LOCK for its full body (see lock comment above). Uses
// `std::env::temp_dir` for a real absolute cross-platform path — `/tmp/...`
// isn't absolute on Windows.
#[test]
fn gguf_dir_precedence_setting_then_env_then_default() {
    let _guard = GGUF_ENV_LOCK.lock().unwrap();
    let env_path = std::env::temp_dir().join("qm-gguf-test");
    let setting_path = std::env::temp_dir().join("qm-models-shared");
    std::env::set_var("QUANTAMIND_GGUF_DIR", &env_path);
    assert_eq!(gguf_dir(), env_path, "env beats default");
    assert_eq!(gguf_dir_resolved(Some(setting_path.to_str().unwrap())), setting_path,
        "setting beats env");
    assert_eq!(gguf_dir_resolved(Some("  ")), env_path,
        "blank setting falls through to env");
    std::env::remove_var("QUANTAMIND_GGUF_DIR");
}

#[test]
fn resolved_setting_wins_without_touching_env() {
    let setting_path = std::env::temp_dir().join("qm-models-shared-2");
    assert_eq!(gguf_dir_resolved(Some(setting_path.to_str().unwrap())), setting_path);
}

#[test]
fn relative_setting_resolves_to_an_absolute_path() {
    // A relative setting (e.g. "./gguf") must never surface as a hidden path.
    let resolved = gguf_dir_resolved(Some("./gguf"));
    assert!(resolved.is_absolute(), "expected absolute, got {resolved:?}");
    assert!(resolved.ends_with("gguf"));
}

#[test]
fn mlx_model_dir_sanitizes_repo_into_a_subdir() {
    let p = mlx_model_dir(Path::new("/m"), "mlx-community/Llama-3.2-3B-Instruct-4bit");
    assert_eq!(p, PathBuf::from("/m/mlx-community_Llama-3.2-3B-Instruct-4bit"));
}

// Holds MLX_ENV_LOCK for its full body (see lock comment above).
#[test]
fn mlx_dir_precedence_setting_then_env_then_default() {
    let _guard = MLX_ENV_LOCK.lock().unwrap();
    let env_path = std::env::temp_dir().join("qm-mlx-test");
    let setting_path = std::env::temp_dir().join("qm-models-mlx");
    std::env::set_var("QUANTAMIND_MLX_DIR", &env_path);
    assert_eq!(mlx_dir(), env_path, "env beats default");
    assert_eq!(mlx_dir_resolved(Some(setting_path.to_str().unwrap())), setting_path,
        "setting beats env");
    assert_eq!(mlx_dir_resolved(Some("  ")), env_path,
        "blank setting falls through to env");
    std::env::remove_var("QUANTAMIND_MLX_DIR");
}

#[test]
fn mlx_dir_default_relative_setting_is_absolute() {
    let resolved = mlx_dir_resolved(Some("./mlx"));
    assert!(resolved.is_absolute(), "expected absolute, got {resolved:?}");
    assert!(resolved.ends_with("mlx"));
}

// Phase 4: on Windows the default lands under %LOCALAPPDATA%\QuantaMind\{gguf,mlx};
// on Unix it stays under ~/.quantamind/{gguf,mlx} — byte-identical to before the
// refactor. Only asserts the LEAF path components (which are stable) so a runner
// that doesn't have %LOCALAPPDATA% or $HOME set falls to a bare relative path
// without failing the test.
#[test]
fn gguf_dir_default_targets_correct_per_os_leaf() {
    let _guard = GGUF_ENV_LOCK.lock().unwrap();
    std::env::remove_var("QUANTAMIND_GGUF_DIR");
    let d = gguf_dir_resolved(None);
    #[cfg(windows)]
    {
        assert!(d.iter().any(|c| c == std::ffi::OsStr::new("QuantaMind")),
            "expected QuantaMind component on Windows, got {d:?}");
    }
    #[cfg(not(windows))]
    {
        assert!(d.iter().any(|c| c == std::ffi::OsStr::new(".quantamind")),
            "expected .quantamind component on Unix, got {d:?}");
    }
    assert_eq!(d.file_name().and_then(|s| s.to_str()), Some("gguf"));
}

#[test]
fn mlx_dir_default_targets_correct_per_os_leaf() {
    let _guard = MLX_ENV_LOCK.lock().unwrap();
    std::env::remove_var("QUANTAMIND_MLX_DIR");
    let d = mlx_dir_resolved(None);
    #[cfg(windows)]
    {
        assert!(d.iter().any(|c| c == std::ffi::OsStr::new("QuantaMind")),
            "expected QuantaMind component on Windows, got {d:?}");
    }
    #[cfg(not(windows))]
    {
        assert!(d.iter().any(|c| c == std::ffi::OsStr::new(".quantamind")),
            "expected .quantamind component on Unix, got {d:?}");
    }
    assert_eq!(d.file_name().and_then(|s| s.to_str()), Some("mlx"));
}
