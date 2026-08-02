#![deny(clippy::unwrap_used)]
use crate::errors::AppError;
use crate::inference::hf::hf_browse::{
    repo_all_files, repo_gguf_files, search_models, HfRepoFile, HfSearchHit, RepoKind,
};

const HF_ENDPOINT: &str = "https://huggingface.co";
const DEFAULT_LIMIT: u32 = 30;

#[tauri::command]
pub async fn hf_search(
    query: String,
    limit: Option<u32>,
    kind: Option<RepoKind>,
) -> Result<Vec<HfSearchHit>, AppError> {
    let kind = kind.unwrap_or(RepoKind::Gguf);
    search_models(HF_ENDPOINT, &query, limit.unwrap_or(DEFAULT_LIMIT), kind).await
}

#[tauri::command]
pub async fn hf_repo_files(repo: String) -> Result<Vec<HfRepoFile>, AppError> {
    repo_gguf_files(HF_ENDPOINT, &repo).await
}

/// All downloadable files in a repo (the the remote server snapshot set), so the UI can show
/// the total download size and a fit estimate before downloading.
#[tauri::command]
pub async fn hf_repo_all_files(repo: String) -> Result<Vec<HfRepoFile>, AppError> {
    repo_all_files(HF_ENDPOINT, &repo).await
}
