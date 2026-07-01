use std::path::Path;
use std::process::Command;

/// Short git commit the binary was built from, stamped into the binary as
/// `QM_BUILD_HASH` (read via `env!`). It rides on the publish payload as
/// build-provenance so the leaderboard can tell two builds at the same crate
/// version apart and dedup/verify submissions. Falls back to `unknown` outside a
/// git checkout (e.g. a source tarball) — never fails the build.
fn emit_build_hash() {
    let hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=QM_BUILD_HASH={hash}");
    // Re-run when HEAD moves (new commit / branch switch) so the stamp stays fresh.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs");
}

fn main() {
    emit_build_hash();

    // `binaries/` is bundled as a Tauri resource (tauri.conf.json) but is
    // gitignored (multi-MB sidecar artifacts: llama-server, the shared
    // libggml-* dylibs whisper.cpp also loads). A fresh clone has no such
    // directory, so Tauri's resource check fails the build before anything else
    // runs. Create it so the build always succeeds, and warn loudly when the
    // sidecar itself is absent so the user knows to fetch it instead of hitting
    // a silent runtime failure.
    let binaries = Path::new("binaries");
    if let Err(e) = std::fs::create_dir_all(binaries) {
        println!("cargo:warning=could not create backend/binaries/: {e}");
    }
    let bin_name = if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    };
    // Re-run whenever the sidecar appears or disappears so the warning clears
    // as soon as fetch-llama-server.sh is run without needing a manual `cargo clean`.
    println!("cargo:rerun-if-changed=binaries/{bin_name}");
    if !binaries.join(bin_name).exists() {
        println!(
            "cargo:warning=backend/binaries/{bin_name} is missing — run \
             scripts/fetch-llama-server.sh before bundling, or llama.cpp \
             features will be unavailable at runtime."
        );
    }

    tauri_build::build()
}
