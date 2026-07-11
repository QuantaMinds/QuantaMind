//! Phase 9-v2: the authored tiered-scenario engine. A v2 collection (one JSON
//! object with `world_state` discovery, wildcard `expected_calls`, `must_not_call`
//! traps) transpiles into the existing `ToolTask`/`AgenticSpec` and runs on the
//! UNCHANGED agentic runner — no second execution path.
pub mod collection;
pub mod env_corpus;
pub mod env_fs;
pub mod env_webui;
pub mod generator;
pub mod r#match;
pub mod noise;
pub mod oracle;
pub mod scenarios;
pub mod transpile;
pub mod world_state;
