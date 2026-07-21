use crate::sync::MutexExt;
use std::sync::{Mutex, OnceLock};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

fn system() -> &'static Mutex<System> {
    static SYS: OnceLock<Mutex<System>> = OnceLock::new();
    SYS.get_or_init(|| Mutex::new(System::new()))
}

/// Total resident memory (bytes) of all processes whose name contains `needle`
/// (lowercased), or `None` when none match. `.memory()` is bytes on sysinfo 0.32.
fn rss_matching(needle: &str) -> Option<u64> {
    let mut sys = system().lock_recover();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::new().with_memory(),
    );
    let total: u64 = sys
        .processes()
        .values()
        .filter(|p| p.name().to_string_lossy().to_lowercase().contains(needle))
        .map(|p| p.memory())
        .sum();
    (total > 0).then_some(total)
}

/// Total resident memory (bytes) of all running `ollama` processes (the server
/// plus its model runner), or `None` if none are running. Sampled per run by
/// the frontend's basic leak heuristic.
pub fn ollama_rss() -> Option<u64> {
    rss_matching("ollama")
}

/// Process RSS of the LOCAL inference server for `kind`, or `None` when it can't be
/// measured honestly: remote backends (vLLM/SGLang — another machine's memory) and any
/// local server whose process isn't found return `None`, never 0. Name-matched (the
/// same heuristic the leak sampler uses), so an externally-started server still counts.
pub fn backend_rss(kind: crate::inference::backend::backend_kind::BackendKind) -> Option<u64> {
    use crate::inference::backend::backend_kind::BackendKind;
    let needle = match kind {
        BackendKind::Ollama => "ollama",
        BackendKind::LlamaCpp => "llama-server",
        BackendKind::Mlx => "mlx_lm",
        BackendKind::VLlm | BackendKind::SgLang => return None,
    };
    rss_matching(needle)
}

#[tauri::command]
pub fn get_ollama_rss() -> Option<u64> {
    ollama_rss()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_rss_never_panics() {
        // Value depends on whether Ollama is running; just exercise the path.
        let _ = ollama_rss();
    }
}
