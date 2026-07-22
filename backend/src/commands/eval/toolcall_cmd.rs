#[cfg(feature = "gui")]
use crate::commands::prompt::prompt_options::{to_generate_options, validate_params};
use crate::errors::AppError;
#[cfg(feature = "gui")]
use crate::inference::backend::backend_kind::BackendKind;
#[cfg(feature = "gui")]
use crate::inference::backend::endpoint;
#[cfg(feature = "gui")]
use crate::persistence::prompts::schema::InferenceParams;
use crate::inference::eval::agentic::v2::scenarios::{is_curated, v2_header, V2_SCENARIOS};
#[cfg(feature = "gui")]
use crate::inference::eval::toolcall::eval::{run_eval_traced, trace_one, ToolCallReport, TraceResult};
use crate::inference::eval::toolcall::tasks::{builtin_collection, ToolTask};
#[cfg(feature = "gui")]
use crate::inference::eval::toolcall::tasks::validate_tasks;
#[cfg(feature = "gui")]
use crate::persistence::eval_trace_store;
use serde::Serialize;
#[cfg(feature = "gui")]
use std::path::PathBuf;
#[cfg(feature = "gui")]
use tauri::Manager;

#[cfg(feature = "gui")]
pub(crate) fn endpoint_for(backend: BackendKind) -> String {
    endpoint::base_url(backend)
}

/// Managed dir for per-collection per-task trace caches (mirrors the `history/`
/// dir). Shared with the matrix command so both runners cache to one place.
#[cfg(feature = "gui")]
pub(crate) fn traces_dir(app: &tauri::AppHandle) -> Result<PathBuf, AppError> {
    let dir = app.path().app_config_dir().map_err(|e| AppError::Io(e.to_string()))?;
    Ok(dir.join("traces"))
}

/// One built-in v2 tiered collection for the picker: the id (file stem), a short
/// humanized domain `label`, and its `tier` — so the UI can group Easy→Extreme and
/// label by domain, while flat dropdowns can still show `label`.
#[derive(Serialize)]
pub struct BuiltinCollectionInfo {
    pub id: String,
    pub label: String,
    pub domain: String,
    pub tier: String,
}

/// Title-case a `-`/`_`-separated identifier ("supply-chain-recon" → "Supply Chain Recon").
fn humanize(s: &str) -> String {
    s.split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The v2 scenario collections OFFERED in the dataset picker: the curated three
/// domains per tier (`is_curated`). The rest of `V2_SCENARIOS` stays bundled and
/// loadable by id — it just isn't listed, so a tier shows three choices instead of a
/// dozen. Single choke point: the app's pickers, the `qm` CLI picker, and the
/// readiness sibling merge all read this.
#[cfg_attr(feature = "gui", tauri::command)]
pub fn list_builtin_collections() -> Vec<BuiltinCollectionInfo> {
    V2_SCENARIOS
        .iter()
        .filter_map(|(id, json)| {
            if !is_curated(id) {
                return None;
            }
            let h = v2_header(json)?;
            let tier = h.tier.to_lowercase();
            // Short domain label: the header's declared domain (humanized) — id-derived
            // only as a fallback, so a VERSIONED id ("medium-coding-v2") still reads
            // "Coding", never "Coding V2" (labels are domain-only by contract).
            let short = if h.domain.is_empty() {
                id.strip_prefix(&format!("{tier}-")).unwrap_or(id).to_string()
            } else {
                h.domain.clone()
            };
            Some(BuiltinCollectionInfo {
                id: id.to_string(),
                label: humanize(&short),
                domain: h.domain,
                tier,
            })
        })
        .collect()
}

/// Tasks for a built-in collection id (a v2 scenario file stem, e.g. "easy-coding").
#[cfg_attr(feature = "gui", tauri::command)]
pub fn get_builtin_collection(id: String) -> Result<Vec<ToolTask>, AppError> {
    builtin_collection(&id).ok_or_else(|| AppError::NotFound(format!("built-in collection '{id}'")))
}

/// Run a tool-call reliability eval over the given `tasks` (built-in or custom)
/// against a model on a backend and return the report. Tasks are validated here
/// too — a command can be invoked directly, so the trust boundary is enforced
/// regardless of source. The endpoint (MLX's dynamic port) is resolved here so
/// the frontend stays port-agnostic. Each task's full trace is cached under
/// `collection_id` (best-effort: a cache-write hiccup never fails the eval — the
/// visualizer falls back to a live run) so "View Trace" needs no re-run.
#[cfg(feature = "gui")]
#[tauri::command]
pub async fn run_toolcall_eval(
    app: tauri::AppHandle,
    model: String,
    backend: Option<BackendKind>,
    collection_id: String,
    tasks: Vec<ToolTask>,
    params: Option<InferenceParams>,
) -> Result<ToolCallReport, AppError> {
    validate_tasks(&tasks)?;
    let backend = backend.unwrap_or_default();
    let options = match &params {
        Some(p) => { validate_params(p)?; Some(to_generate_options(p)) }
        None => None,
    };
    let (report, traces) = run_eval_traced(backend, &endpoint_for(backend), &model, &tasks, options).await?;
    // Empty id = a probe that doesn't need a drill-down (context-cliff, quant
    // sweep) — skip caching. Otherwise cache best-effort (a write hiccup never
    // fails the eval; the visualizer falls back to a live run).
    if !collection_id.is_empty() {
        if let Ok(dir) = traces_dir(&app) {
            let _ = eval_trace_store::upsert(&dir, &collection_id, &model, backend, &traces);
        }
    }
    Ok(report)
}

/// The cached trace for one `(collection, model, task)` from the last run, or
/// `None` if never run/saved — so the pipeline visualizer shows saved data
/// without re-running inference.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn load_toolcall_trace(
    app: tauri::AppHandle,
    collection_id: String,
    model: String,
    task_id: String,
) -> Result<Option<TraceResult>, AppError> {
    eval_trace_store::load_one(&traces_dir(&app)?, &collection_id, &model, &task_id)
}

/// Trace ONE task end-to-end for the pipeline visualizer: the exact system
/// message sent, the model's raw output, and the verdict — so the eval isn't a
/// black box. Same trust boundary (validates the task) and endpoint resolution.
#[cfg(feature = "gui")]
#[tauri::command]
pub async fn trace_toolcall_task(
    model: String,
    backend: Option<BackendKind>,
    task: ToolTask,
) -> Result<TraceResult, AppError> {
    validate_tasks(std::slice::from_ref(&task))?;
    let backend = backend.unwrap_or_default();
    trace_one(backend, &endpoint_for(backend), &model, &task, None).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// What the pickers actually receive: twelve collections, three per tier — never a
    /// fixture-only one (fs / web-UI / corpus / noise) and never a Category K probe,
    /// which stay bundled for the engine tests and the CLI but must not be offered.
    #[test]
    fn the_picker_lists_exactly_three_collections_per_tier() {
        let infos = list_builtin_collections();
        let mut per_tier: HashMap<&str, usize> = HashMap::new();
        for i in &infos {
            *per_tier.entry(i.tier.as_str()).or_default() += 1;
        }
        for tier in ["easy", "medium", "hard", "extreme"] {
            assert_eq!(per_tier.get(tier).copied().unwrap_or(0), 3, "tier '{tier}' should offer 3 collections");
        }
        assert_eq!(infos.len(), 12);
        for hidden in ["easy-coding-fs", "easy-webui-tasks", "easy-research-search", "noisy-extraction", "boundary-banking", "boundary-context-squeeze"] {
            assert!(!infos.iter().any(|i| i.id == hidden), "unlisted '{hidden}' must not be offered");
            // ...but it stays LOADABLE by id, so `qm run --collection <id>` and any saved
            // run still work — hidden from the picker is not removed from the engine.
            assert!(get_builtin_collection(hidden.to_string()).is_ok(), "unlisted '{hidden}' must still load by id");
        }
    }

    /// Labels stay domain-only (the tier prefix is stripped), so a tier group reads as
    /// three domains rather than three near-identical ids.
    #[test]
    fn a_listed_collection_is_labeled_by_domain_not_by_id() {
        let infos = list_builtin_collections();
        let coding = infos.iter().find(|i| i.id == "medium-coding-v2").expect("medium-coding-v2 is offered");
        assert_eq!(coding.label, "Coding");
        assert_eq!(coding.tier, "medium");
    }
}
