use crate::errors::{AppError, AppResult};
use crate::inference::hf::hf_request::{build_client, build_url, map_status, validate_repo};
use crate::inference::hf::hf_resume::{decide, local_size, partial_path, ResumeStrategy};
use crate::inference::pull::pull_speed::SpeedTracker;
use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct DownloadProgress {
    pub bytes_completed: u64,
    pub bytes_total: u64,
    pub speed_bps: u64,
}

#[derive(Debug, Clone)]
pub struct DownloadResult {
    pub final_path: PathBuf,
    pub sha256: Option<String>,
}

fn parse_content_length(h: Option<&reqwest::header::HeaderValue>, repo: &str) -> AppResult<u64> {
    let v = h.ok_or_else(|| AppError::Inference(format!("{repo}: missing Content-Length")))?;
    let s = v.to_str().map_err(|e| AppError::Inference(format!("{repo}: non-ASCII Content-Length: {e}")))?;
    s.parse().map_err(|e| AppError::Inference(format!("{repo}: unparseable Content-Length '{s}': {e}")))
}

/// HuggingFace exposes an LFS object's SHA-256 as the `X-Linked-Etag` on the resolve
/// endpoint (git-lfs `oid`). We use it as the expected digest to verify the download against.
/// `None` for a non-LFS file (small config/tokenizer) — then we record the computed hash but
/// have nothing authoritative to compare it to. Quotes/`W/` weak-validator prefixes stripped;
/// only a 64-char lowercase hex is accepted.
fn expected_sha256(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let raw = headers.get("x-linked-etag").or_else(|| headers.get("etag"))?;
    let s = raw.to_str().ok()?;
    let s = s.trim().trim_start_matches("W/").trim_matches('"').to_ascii_lowercase();
    (s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())).then_some(s)
}

/// Download a single `.gguf` file. Thin guard over [`download_file`].
pub async fn download_gguf(
    endpoint: &str,
    repo: &str,
    filename: &str,
    dest_path: &Path,
    on_progress: impl Fn(DownloadProgress),
    cancel: CancellationToken,
) -> AppResult<DownloadResult> {
    if !filename.to_lowercase().ends_with(".gguf") {
        return Err(AppError::Validation(format!("not a .gguf filename: {filename}")));
    }
    download_file(endpoint, repo, filename, dest_path, on_progress, cancel).await
}

/// Stream one repo file to `dest_path` with `.partial` resume and cancellation.
/// Format-agnostic — the GGUF guard lives in [`download_gguf`]; snapshots call
/// this directly for each file.
pub async fn download_file(
    endpoint: &str,
    repo: &str,
    filename: &str,
    dest_path: &Path,
    on_progress: impl Fn(DownloadProgress),
    cancel: CancellationToken,
) -> AppResult<DownloadResult> {
    validate_repo(repo)?;
    if dest_path.exists() {
        return Ok(DownloadResult { final_path: dest_path.to_path_buf(), sha256: None });
    }
    let url = build_url(endpoint, repo, filename);
    let client = build_client()?;

    let head = client.head(&url).send().await.map_err(|e| AppError::Inference(e.to_string()))?;
    if let Some(err) = map_status(head.status(), repo) { return Err(err); }
    let total = parse_content_length(head.headers().get("content-length"), repo)?;
    let expected_sha = expected_sha256(head.headers());

    let partial = partial_path(dest_path);
    if matches!(decide(local_size(&partial), total), ResumeStrategy::RedownloadAfterDelete) {
        fs::remove_file(&partial).map_err(|e| AppError::Io(e.to_string()))?;
    }

    let strategy = decide(local_size(&partial), total);
    if strategy == ResumeStrategy::Skip {
        fs::rename(&partial, dest_path).map_err(|e| AppError::Io(e.to_string()))?;
        return Ok(DownloadResult { final_path: dest_path.to_path_buf(), sha256: None });
    }
    let start = if let ResumeStrategy::Resume(n) = strategy { n } else { 0 };

    let mut req = client.get(&url);
    if start > 0 { req = req.header("Range", format!("bytes={start}-")); }
    let resp = req.send().await.map_err(|e| AppError::Inference(e.to_string()))?;
    if let Some(err) = map_status(resp.status(), repo) { return Err(err); }

    let mut file = OpenOptions::new().create(true).append(true).open(&partial).map_err(|e| AppError::Io(e.to_string()))?;
    // Hash the whole file as it lands so we can verify integrity (rule: no unverified artifacts).
    // On resume, fold the already-downloaded prefix in first so the digest covers all bytes.
    let mut hasher = Sha256::new();
    if start > 0 {
        hash_existing_prefix(&partial, start, &mut hasher)?;
    }
    let mut speed = SpeedTracker::new(Duration::from_secs(5));
    let mut completed = start;
    speed.add(Instant::now(), completed);
    let mut bytes = resp.bytes_stream();

    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return Ok(DownloadResult { final_path: partial.clone(), sha256: None }),
            chunk = bytes.next() => {
                let Some(chunk) = chunk else { break };
                let chunk = chunk.map_err(|e| AppError::Inference(e.to_string()))?;
                if completed + chunk.len() as u64 > total {
                    return Err(AppError::Inference(format!("{repo}: server sent more bytes than Content-Length ({} > {total})", completed + chunk.len() as u64)));
                }
                file.write_all(&chunk).map_err(|e| AppError::Io(e.to_string()))?;
                hasher.update(&chunk);
                completed += chunk.len() as u64;
                speed.add(Instant::now(), completed);
                on_progress(DownloadProgress { bytes_completed: completed, bytes_total: total, speed_bps: speed.bps(Instant::now()) });
            }
        }
    }
    drop(file);

    // Truncation guard: a stream that ends early would otherwise rename a short file as complete.
    if completed != total {
        return Err(AppError::Inference(format!(
            "{repo}: download truncated ({completed} of {total} bytes) — leaving .partial for resume"
        )));
    }
    let digest = format!("{:x}", hasher.finalize());
    // Verify against HF's advertised LFS digest when present. A mismatch means corruption or a
    // substituted body → delete the bad file so a poisoned artifact never loads.
    if let Some(expected) = &expected_sha {
        if &digest != expected {
            let _ = fs::remove_file(&partial);
            return Err(AppError::Inference(format!(
                "{repo}: integrity check FAILED for {filename} (expected {expected}, got {digest}); deleted"
            )));
        }
    }
    fs::rename(&partial, dest_path).map_err(|e| AppError::Io(e.to_string()))?;
    Ok(DownloadResult { final_path: dest_path.to_path_buf(), sha256: Some(digest) })
}

/// Fold the first `len` bytes of an existing `.partial` into `hasher` so a resumed download's
/// digest covers the prefix that was already written on an earlier run.
fn hash_existing_prefix(partial: &Path, len: u64, hasher: &mut Sha256) -> AppResult<()> {
    let mut f = fs::File::open(partial).map_err(|e| AppError::Io(e.to_string()))?;
    let mut remaining = len;
    let mut buf = vec![0u8; 1 << 20];
    while remaining > 0 {
        let want = remaining.min(buf.len() as u64) as usize;
        let n = f.read(&mut buf[..want]).map_err(|e| AppError::Io(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        remaining -= n as u64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

    const HEX64: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(HeaderName::from_static(k), HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn reads_lfs_sha_from_x_linked_etag() {
        assert_eq!(expected_sha256(&headers(&[("x-linked-etag", HEX64)])), Some(HEX64.into()));
    }

    #[test]
    fn strips_quotes_and_weak_validator_and_lowercases() {
        let quoted = format!("W/\"{}\"", HEX64.to_ascii_uppercase());
        assert_eq!(expected_sha256(&headers(&[("etag", &quoted)])), Some(HEX64.into()));
    }

    #[test]
    fn rejects_non_sha_etag() {
        // A normal (non-LFS) ETag is not a 64-char hex → we have nothing authoritative.
        assert_eq!(expected_sha256(&headers(&[("etag", "\"abc-123-not-a-sha\"")])), None);
        assert_eq!(expected_sha256(&headers(&[("x-linked-etag", "deadbeef")])), None);
        assert_eq!(expected_sha256(&HeaderMap::new()), None);
    }

    #[test]
    fn prefers_x_linked_etag_over_plain_etag() {
        let other = "1111111111111111111111111111111111111111111111111111111111111111";
        let h = headers(&[("x-linked-etag", HEX64), ("etag", other)]);
        assert_eq!(expected_sha256(&h), Some(HEX64.into()));
    }
}
