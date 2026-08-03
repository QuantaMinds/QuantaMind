// OS-specific platform adapters. Each concrete impl lives in its own file and
// is `#[cfg(target_os = "...")]`-gated on the mod declaration below — so a
// macOS build literally contains no Windows or Linux code, a Windows build
// contains no macOS or Linux code, and so on. Result: smaller binaries and
// OS-specific error diagnostics stay OS-specific.
//
// Every runtime-engine lifecycle module (`commands/llama/…_runtime.rs`,
// plus `commands/app_lifecycle.rs`) uses `os::Host::…` instead of scattering
// `#[cfg(target_os = "…")]` blocks. Adding a new engine = one adapter impl.

pub mod engine_host;
pub mod host;
/// Stop a child's whole process group. Cross-cutting because every spawner needs it:
/// the MCP transport, and the `qm certify` agent actuator.
pub mod proc_group;
/// A temp directory that removes itself, and reaps the ones a killed run left behind.
/// Here rather than beside one caller because both the MCP world and the certify
/// harness need it, and the orphan sweep is inherently OS work.
pub mod scratch_dir;
pub mod user_dirs;

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "windows")]
pub mod windows;

pub use engine_host::EngineHost;
pub use host::Host;
pub use scratch_dir::ScratchDir;
