use crate::commands::stt::stt_runtime::{
    bin_name, build_spawn_args, is_reachable, is_ready, spawn_server, POLL_INTERVAL_MS, PORT,
    PROBE_TIMEOUT_MS, READY_TIMEOUT_SECS,
};
use crate::commands::settings::user_settings::UserSettingsState;
use crate::commands::stt::stt_server_types::{SttServerState, SttStartResult};
use crate::errors::AppError;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::Manager;

pub const NOT_BUNDLED_MSG: &str =
    "whisper.cpp (whisper-server) isn't installed. Install it with `brew install whisper-cpp`, then Re-check.";
pub const READY_TIMEOUT_MSG: &str =
    "whisper-server started but didn't report a loaded model within 30 seconds.";

/// Directory holding a runnable `whisper-server`. Resolution order, most
/// explicit first: persistent user setting → `QUANTAMIND_WHISPER_DIR` env →
/// PATH/Homebrew (a `brew install whisper-cpp`) → bundled resources (prod) →
/// source tree (dev). The PATH/Homebrew step is what makes a user-installed
/// engine "just work" without any path juggling. `spawn_server`'s
/// `current_dir` + `DYLD_FALLBACK_LIBRARY_PATH` let the brew binary resolve its
/// `@loader_path/../lib` `libwhisper`.
pub(crate) fn whisper_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    let setting = app
        .try_state::<UserSettingsState>()
        .and_then(|s| s.stt_engine_dir(app).ok().flatten());
    let env = std::env::var("QUANTAMIND_WHISPER_DIR").ok();
    let explicit: Vec<PathBuf> = [setting, env]
        .into_iter()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .collect();
    if let Some(d) = first_dir_with_bin(&explicit) {
        return Some(d);
    }
    if let Some(d) = resolve_whisper_on_path() {
        return Some(d);
    }
    if let Ok(res) = app.path().resource_dir() {
        if let Some(d) = has_bin(res.join("binaries")) {
            return Some(d);
        }
    }
    #[cfg(debug_assertions)]
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dev) = exe.parent().and_then(|p| p.parent()).and_then(|p| p.parent()) {
            return has_bin(dev.join("binaries"));
        }
    }
    None
}

fn has_bin(dir: PathBuf) -> Option<PathBuf> {
    dir.join(bin_name()).exists().then_some(dir)
}

/// First of `dirs` that actually contains the `whisper-server` binary. Pure, so
/// the setting-before-env ordering is testable without Tauri.
fn first_dir_with_bin(dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter().find_map(|d| has_bin(d.clone()))
}

/// Discover a user-installed `whisper-server` on PATH or in each OS's standard
/// install prefixes, mirroring `ollama_runtime::resolve_ollama`. The hardcoded
/// prefixes matter: a Finder / Explorer / Files-launched GUI app doesn't
/// inherit the shell `PATH`, so `which` / `where.exe` alone would miss a
/// brew / winget / apt install.
fn resolve_whisper_on_path() -> Option<PathBuf> {
    use crate::os::{EngineHost, Host};
    if let Some(parent) = Host::resolve_on_path(bin_name()) {
        if let Some(d) = has_bin(parent) {
            return Some(d);
        }
    }
    for dir in whisper_fallback_dirs() {
        if let Some(d) = has_bin(dir) {
            return Some(d);
        }
    }
    None
}

/// Per-OS well-known install prefixes for whisper.cpp. macOS: Homebrew.
/// Windows: `%LOCALAPPDATA%\Programs\whisper-cpp` (winget default) + Scoop
/// shims. Linux: `~/.local/bin`, `/usr/local/bin`, `/usr/bin`.
fn whisper_fallback_dirs() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        vec![PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin")]
    }
    #[cfg(target_os = "windows")]
    {
        let mut v = Vec::new();
        if let Ok(lad) = std::env::var("LOCALAPPDATA") {
            v.push(PathBuf::from(&lad).join("Programs").join("whisper-cpp"));
        }
        if let Ok(up) = std::env::var("USERPROFILE") {
            v.push(PathBuf::from(&up).join("scoop").join("shims"));
            v.push(
                PathBuf::from(&up)
                    .join("scoop")
                    .join("apps")
                    .join("whisper-cpp")
                    .join("current"),
            );
        }
        v
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut v = Vec::new();
        if let Ok(home) = std::env::var("HOME") {
            v.push(PathBuf::from(&home).join(".local").join("bin"));
        }
        v.push(PathBuf::from("/usr/local/bin"));
        v.push(PathBuf::from("/usr/bin"));
        v
    }
}

/// Whether the STT engine is present AND actually runnable in this environment.
/// `found` = a `whisper-server` binary was located; `runnable` = it executed (a
/// `--help` dry-run exited 0). The split exists because a binary can be present
/// yet broken — e.g. its `libwhisper` dylib is missing/mismatched — and we must
/// not signal "ready" then fail on start. `error` carries the diagnostic for
/// the not-runnable case.
#[derive(Serialize, Debug, PartialEq)]
pub struct WhisperEnv {
    pub found: bool,
    pub dir: Option<String>,
    pub runnable: bool,
    pub error: Option<String>,
}

/// Prove `whisper-server` actually runs from `dir` (its dylibs resolve), not just
/// that the file exists: run `--help` with the same env `spawn_server` uses. A
/// broken install exits non-zero with a dyld `Library not loaded: …` line on
/// stderr (macOS), a `ld.so: cannot open shared object` line (Linux), or a
/// `0xC0000135` exit (Windows, missing DLL) — we return the diagnostic tail.
fn dry_run(dir: &Path) -> Result<(), String> {
    use crate::os::{EngineHost, Host};
    let mut cmd = Host::command(dir.join(bin_name()));
    cmd.arg("--help").current_dir(dir);
    for (k, v) in Host::envs_for_lib_dir(dir) {
        cmd.env(k, v);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let tail: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = tail.iter().rev().take(4).rev().cloned().collect::<Vec<_>>().join("\n");
    Err(if tail.trim().is_empty() {
        format!("whisper-server couldn't run (exit {})", out.status)
    } else {
        tail
    })
}

/// Report whether the whisper.cpp STT engine is installed and runnable, so the
/// UI can show the catalog (ready) vs a setup card (install / reinstall) without
/// attempting a start. The dry-run runs off the async runtime.
#[tauri::command]
pub async fn check_whisper_env(app: tauri::AppHandle) -> WhisperEnv {
    let Some(dir) = whisper_dir(&app) else {
        return WhisperEnv { found: false, dir: None, runnable: false, error: None };
    };
    let dir_str = dir.to_string_lossy().into_owned();
    let probe = dir.clone();
    let result = tokio::task::spawn_blocking(move || dry_run(&probe)).await;
    match result {
        Ok(Ok(())) => WhisperEnv { found: true, dir: Some(dir_str), runnable: true, error: None },
        Ok(Err(e)) => {
            WhisperEnv { found: true, dir: Some(dir_str), runnable: false, error: Some(e) }
        }
        Err(e) => WhisperEnv {
            found: true,
            dir: Some(dir_str),
            runnable: false,
            error: Some(format!("dry-run failed to run: {e}")),
        },
    }
}

/// The R2 ownership decision from the two probe facts. Pure so the truth table
/// is unit-testable without async or Tauri state.
#[derive(Debug, PartialEq)]
enum Adopt {
    /// Our live server already serves this model — nothing to do.
    AlreadyOurs,
    /// The port answers but it isn't ours — fatal, never adopt a stranger.
    Conflict,
    /// Free, or ours-but-different-model: stop-if-needed and (re)spawn.
    Proceed,
}

fn adopt_decision(ours_alive: bool, ours_model: bool, reachable: bool) -> Adopt {
    if ours_alive && ours_model {
        Adopt::AlreadyOurs
    } else if !ours_alive && reachable {
        Adopt::Conflict
    } else {
        Adopt::Proceed
    }
}

fn port_conflict_note() -> String {
    format!(
        "Something is already using the STT port {PORT}. Stop it and try again — \
         QuantaMind won't take over a process it didn't start."
    )
}

/// Pure pre-spawn gate over the resolved bundle dir + the two model paths.
/// `Ok(dir)` to proceed; `Err(result)` for a gate the user must resolve. The
/// VAD presence gate is mandatory — without it the silence-hallucination metric
/// would silently disable.
fn precheck_spawn<'a>(
    dir: Option<&'a Path>,
    model_path: &str,
    vad_path: &str,
) -> Result<&'a Path, SttStartResult> {
    let Some(dir) = dir else {
        return Err(SttStartResult::NotBundled { note: NOT_BUNDLED_MSG.into() });
    };
    if !Path::new(model_path).exists() {
        return Err(SttStartResult::ModelMissing {
            note: format!("The whisper model file is missing: {model_path}. Download it first."),
        });
    }
    if !Path::new(vad_path).exists() {
        return Err(SttStartResult::VadMissing {
            note: format!(
                "The silero VAD model is missing: {vad_path}. Re-run the download — \
                 the VAD ships together with the whisper model."
            ),
        });
    }
    Ok(dir)
}

/// Poll `/health` until the model is loaded (HTTP 200), bailing the instant our
/// child dies so a crash surfaces its stderr tail instead of a 30s wait.
async fn await_ready(state: &SttServerState) -> bool {
    let attempts = (READY_TIMEOUT_SECS * 1000) / POLL_INTERVAL_MS;
    for _ in 0..attempts {
        tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
        if !state.is_alive() {
            return false;
        }
        if is_ready(PROBE_TIMEOUT_MS).await {
            return true;
        }
    }
    false
}

#[tauri::command]
pub async fn start_whisper_server(
    app: tauri::AppHandle,
    state: tauri::State<'_, SttServerState>,
    model_path: String,
    vad_path: String,
) -> Result<SttStartResult, AppError> {
    let ours_alive = state.is_alive();
    let ours_model = state.is_model(&model_path);
    let reachable = is_reachable(PROBE_TIMEOUT_MS).await;
    match adopt_decision(ours_alive, ours_model, reachable) {
        Adopt::AlreadyOurs => return Ok(SttStartResult::AlreadyRunning),
        Adopt::Conflict => return Ok(SttStartResult::PortConflict { note: port_conflict_note() }),
        Adopt::Proceed => {}
    }
    // Stop our previous (different-model or dead) server before (re)spawning.
    state.stop().map_err(AppError::Internal)?;
    let dir = whisper_dir(&app);
    let dir = match precheck_spawn(dir.as_deref(), &model_path, &vad_path) {
        Ok(d) => d.to_path_buf(),
        Err(gate) => return Ok(gate),
    };
    let (child, tail) = match spawn_server(&dir, &build_spawn_args(&model_path, &vad_path, PORT)) {
        Ok(ct) => ct,
        Err(error) => return Ok(SttStartResult::StartFailed { error, stderr_tail: String::new() }),
    };
    let pid = child.id();
    state.store(child, model_path, vad_path, tail);
    if await_ready(&state).await {
        Ok(SttStartResult::Started { pid, port: PORT })
    } else {
        let stderr_tail = state.tail_snapshot();
        let _ = state.stop();
        Ok(SttStartResult::StartFailed { error: READY_TIMEOUT_MSG.into(), stderr_tail })
    }
}

#[tauri::command]
pub async fn stop_whisper_server(
    state: tauri::State<'_, SttServerState>,
) -> Result<(), AppError> {
    state.stop().map_err(AppError::Internal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::stt::stt_runtime::bin_name;

    #[test]
    fn has_bin_requires_the_binary_in_the_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(has_bin(dir.path().to_path_buf()).is_none(), "empty dir resolves to None");
        std::fs::write(dir.path().join(bin_name()), b"x").unwrap();
        assert_eq!(has_bin(dir.path().to_path_buf()).as_deref(), Some(dir.path()));
    }

    #[test]
    fn first_dir_with_bin_prefers_the_earliest_dir_that_has_the_binary() {
        let empty = tempfile::tempdir().unwrap();
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        std::fs::write(a.path().join(bin_name()), b"x").unwrap();
        std::fs::write(b.path().join(bin_name()), b"x").unwrap();
        assert!(first_dir_with_bin(&[empty.path().to_path_buf()]).is_none());
        // The earlier candidate (e.g. the user setting) wins over a later one (env).
        assert_eq!(
            first_dir_with_bin(&[a.path().to_path_buf(), b.path().to_path_buf()]).as_deref(),
            Some(a.path())
        );
        // Skips a candidate that lacks the binary.
        assert_eq!(
            first_dir_with_bin(&[empty.path().to_path_buf(), b.path().to_path_buf()]).as_deref(),
            Some(b.path())
        );
    }

    #[test]
    fn adopt_decision_truth_table() {
        // ours, alive, same model -> already ours
        assert_eq!(adopt_decision(true, true, true), Adopt::AlreadyOurs);
        // our live server on a *different* model -> proceed (switch), not a conflict
        assert_eq!(adopt_decision(true, false, true), Adopt::Proceed);
        // port answers but no live child of ours -> foreign, fatal conflict
        assert_eq!(adopt_decision(false, false, true), Adopt::Conflict);
        assert_eq!(adopt_decision(false, true, true), Adopt::Conflict);
        // nothing on the port -> proceed
        assert_eq!(adopt_decision(false, false, false), Adopt::Proceed);
    }

    #[test]
    fn precheck_not_bundled_when_dir_missing() {
        let err = precheck_spawn(None, "/m/model.bin", "/m/vad.bin").unwrap_err();
        assert!(matches!(err, SttStartResult::NotBundled { .. }));
    }

    #[test]
    fn precheck_model_missing_then_vad_missing_then_ok() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("ggml-tiny.en.bin");
        let vad = dir.path().join("ggml-silero-v6.2.0.bin");

        // model absent -> ModelMissing
        let err = precheck_spawn(Some(dir.path()), model.to_str().unwrap(), vad.to_str().unwrap())
            .unwrap_err();
        assert!(matches!(err, SttStartResult::ModelMissing { .. }));

        // model present, vad absent -> VadMissing (the silence metric gate)
        std::fs::write(&model, b"x").unwrap();
        let err = precheck_spawn(Some(dir.path()), model.to_str().unwrap(), vad.to_str().unwrap())
            .unwrap_err();
        assert!(matches!(err, SttStartResult::VadMissing { .. }), "VAD presence gates ready");

        // both present -> Ok(dir)
        std::fs::write(&vad, b"x").unwrap();
        let ok = precheck_spawn(Some(dir.path()), model.to_str().unwrap(), vad.to_str().unwrap());
        assert_eq!(ok.unwrap(), dir.path());
    }

    #[test]
    fn port_conflict_note_names_the_port_and_refuses_adoption() {
        let note = port_conflict_note();
        assert!(note.contains("8093"));
        assert!(note.contains("didn't start"));
    }

    #[cfg(unix)]
    #[test]
    fn dry_run_passes_a_runnable_binary_and_captures_a_broken_one() {
        use std::os::unix::fs::PermissionsExt;
        let make = |dir: &Path, body: &str| {
            let bin = dir.join(bin_name());
            std::fs::write(&bin, body).unwrap();
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        let ok = tempfile::tempdir().unwrap();
        make(ok.path(), "#!/bin/sh\nexit 0\n");
        assert!(dry_run(ok.path()).is_ok(), "a binary that runs is OK");

        let bad = tempfile::tempdir().unwrap();
        make(bad.path(), "#!/bin/sh\necho 'dyld: Library not loaded: @rpath/libwhisper.1.dylib' >&2\nexit 134\n");
        let err = dry_run(bad.path()).unwrap_err();
        assert!(err.contains("Library not loaded"), "captures the dyld diagnostic: {err}");
    }
}
