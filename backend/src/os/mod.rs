// OS-specific platform adapters. Each concrete impl lives in its own file and
// is `#[cfg(target_os = "...")]`-gated on the mod declaration below — so a
// macOS build literally contains no Windows or Linux code, a Windows build
// contains no macOS or Linux code, and so on. Result: smaller binaries and
// OS-specific error diagnostics stay OS-specific.
//
// Every runtime-engine lifecycle module (`commands/{ollama,llama,mlx}/…_runtime.rs`,
// plus `commands/app_lifecycle.rs`) uses `os::Host::…` instead of scattering
// `#[cfg(target_os = "…")]` blocks. Adding a new engine = one adapter impl.

pub mod engine_host;
pub mod host;
pub mod user_dirs;

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "windows")]
pub mod windows;

pub use engine_host::EngineHost;
pub use host::Host;
