//! Phase 7 readiness synthesis: turn a model's measured agentic metrics into a
//! transparent Ready / Conditional / NotReady verdict against a tunable profile.
//! Pure and Tauri-free — the GUI command and the future CLI share `assess`.
pub mod hardware;
pub mod inputs;
pub mod profile;
pub mod recommend;
pub mod rightsizing;
pub mod types;
pub mod verdict;
pub mod vram_fit;
