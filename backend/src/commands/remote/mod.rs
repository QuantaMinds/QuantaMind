// Commands for the remote OpenAI-compatible backends (vLLM, SGLang). They share
// all logic — health probe and model discovery over `/v1/models` — and differ
// only by which `remote_config` slot and `BackendKind` they carry, so the two
// backends live in one module rather than two near-identical dirs.
pub mod remote_health;
#[cfg(feature = "gui")]
pub mod remote_models;
