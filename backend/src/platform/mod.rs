// Platform adapter: one trait, per-OS impls, single compile-time-selected
// `Host` alias. Every runtime-engine lifecycle module (Ollama, llama.cpp,
// whisper, and any future engine) goes through this seam instead of
// sprinkling `#[cfg(target_os = "…")]` blocks — see
// `docs/architecture.md#layering` (dependency law) and the plan's Live-only
// risks R1/R2 for why the Windows impl matters.

pub mod engine_host;
pub mod host;
pub mod user_dirs;

#[cfg(unix)]
pub mod unix_host;
#[cfg(windows)]
pub mod windows_host;

pub use engine_host::EngineHost;
pub use host::Host;
