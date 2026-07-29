use serde::{Deserialize, Serialize};

// The type lives in the domain (`inference/params.rs`) per the layering law —
// the eval engine stamps it on reports — re-exported here for the storage/IPC
// call sites that address it by its persistence-schema path.
pub use crate::inference::params::InferenceParams;

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct PromptFile {
    pub name: String,
    #[serde(default)]
    pub system: String,
    #[serde(default)]
    pub user: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    // Legacy only: prompt files no longer persist params (global params are the
    // single source — see the frontend paramsStore). Read-tolerant so an old
    // file with a `params` block still loads; never written back.
    #[serde(default, skip_serializing)]
    pub params: InferenceParams,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub auto_rerun: bool,
}

pub(crate) fn is_false(b: &bool) -> bool { !*b }

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
