use crate::commands::llama::llama_runtime::kill_server;
use crate::sync::MutexExt;
use serde::Serialize;
use std::process::Child;
use std::sync::Mutex;

/// Outcome of a `start_llama_server` call. Tagged by `status` so the frontend
/// can branch without positional decoding (mirrors `the serverStartResult`).
#[derive(Serialize, Debug, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LlamaStartResult {
    AlreadyRunning,
    /// `note` carries a user-facing message ONLY when a hardware constraint was applied at
    /// launch (flash attention / Q8 KV cache / capped context on a memory-tight host) — so the
    /// UI can tell the user what was detected and how the server is running safely. `None` on a
    /// roomy machine that launched at full precision.
    Started {
        pid: u32,
        port: u16,
        #[serde(skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    NotBundled { note: String },
    StartFailed { error: String },
}

/// One-time spawn readout for the running llama-server. llama.cpp loads the model
/// once at spawn and keeps it resident, so this is NOT a per-request phase: it's
/// the model's on-disk footprint and the wall-clock it took to become ready.
/// `model_bytes` is `None` if the GGUF can't be stat'd; `load_ms` is the
/// spawn→`/health`-ready window (coarse — bounded by the 500ms readiness poll).
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
pub struct SpawnReadout {
    pub model_bytes: Option<u64>,
    pub load_ms: u64,
}

struct RunningServer {
    child: Child,
    model_path: String,
    /// The `-c` the server was launched with. llama.cpp fixes context at launch,
    /// so a request for a different window must relaunch — tracked here so
    /// `is_current` can tell "same model, new context" apart from a no-op start.
    ctx: u32,
    /// KV-cache precision from the `LaunchPlan` ("f16" | "q8_0") — stamped onto batch
    /// reports as run config. Only OUR spawn knows this; an externally-started server
    /// never reaches `store`, so the stamp stays `None` there (never guessed).
    kv_cache_type: &'static str,
    readout: Option<SpawnReadout>,
}

/// Whether the running `llama-server` can serve a context-cliff probe of a given
/// model. The probe never relaunches (the server is user-managed), so it must check
/// — against the EXACT GGUF path the server was launched with — that the right model
/// is loaded and its launch `-c` is wide enough, before marching the ladder into
/// per-rung HTTP 400s or, worse, scoring the wrong model's weights.
#[derive(Debug, PartialEq, Eq)]
pub enum LlamaProbeReadiness {
    /// No server is up.
    NotRunning,
    /// A server is up but loaded a different GGUF than the probe targets.
    WrongModel,
    /// The targeted model is loaded; `ctx` is its launch `-c` (already hardware-clamped).
    Ready { ctx: u32 },
}

/// The single active `llama-server` process. One server per loaded GGUF; a new
/// model stops the previous one (`future-considerations.md` tracks multi-server).
#[derive(Default)]
pub struct LlamaServerState {
    inner: Mutex<Option<RunningServer>>,
}

impl LlamaServerState {
    /// True when the running server is the same GGUF launched with the same `-c`.
    /// A context change (the user's `num_ctx` param) is NOT current — llama.cpp
    /// can only adopt the new window by relaunching.
    pub fn is_current(&self, model_path: &str, ctx: u32) -> bool {
        self.inner
            .lock_recover()
            .as_ref()
            .is_some_and(|s| s.model_path == model_path && s.ctx == ctx)
    }

    /// Whether the running server can serve a cliff probe of `model_path`, matched on
    /// the EXACT launch path (same identity `is_current` uses). The caller compares
    /// `Ready.ctx` against the probe's needed window — a relaunch is the user's job.
    pub fn probe_readiness(&self, model_path: &str) -> LlamaProbeReadiness {
        match self.inner.lock_recover().as_ref() {
            None => LlamaProbeReadiness::NotRunning,
            Some(s) if s.model_path != model_path => LlamaProbeReadiness::WrongModel,
            Some(s) => LlamaProbeReadiness::Ready { ctx: s.ctx },
        }
    }

    /// The running server's `(model_path, launch ctx)`, if any — so the Inspector can surface
    /// the loaded llama.cpp model (a placement API only knows models).
    pub fn running_summary(&self) -> Option<(String, u32)> {
        self.inner.lock_recover().as_ref().map(|s| (s.model_path.clone(), s.ctx))
    }

    pub fn store(&self, child: Child, model_path: String, ctx: u32, kv_cache_type: &'static str) {
        *self.inner.lock_recover() = Some(RunningServer { child, model_path, ctx, kv_cache_type, readout: None });
    }

    /// The running server's launched KV-cache precision ("f16" | "q8_0"), or `None` when no
    /// server WE spawned is up (an external server's flags are unknowable — never guessed).
    pub fn kv_cache_type(&self) -> Option<String> {
        self.inner.lock_recover().as_ref().map(|s| s.kv_cache_type.to_string())
    }

    /// Record the spawn readout once the server is ready. No-op if nothing is
    /// running, so a failed start never leaves a fabricated number.
    pub fn set_readout(&self, readout: SpawnReadout) {
        if let Some(s) = self.inner.lock_recover().as_mut() {
            s.readout = Some(readout);
        }
    }

    /// The current server's spawn readout — `None` when no server is up or it
    /// never became ready.
    pub fn readout(&self) -> Option<SpawnReadout> {
        self.inner.lock_recover().as_ref().and_then(|s| s.readout)
    }

    /// Kill and forget the running server, if any. Idempotent.
    pub fn stop(&self) -> Result<(), String> {
        let running = self.inner.lock_recover().take();
        if let Some(mut s) = running {
            kill_server(&mut s.child)?;
        }
        Ok(())
    }
}
