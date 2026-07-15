//! SecureSecrets — the single port for every at-rest secret (rule 7a).
//!
//! All secrets (cloud API keys, and later the publish refresh token) live in the OS
//! keychain via `keyring`, NEVER plaintext on disk. A per-key in-memory session copy is
//! written first so a keychain that is later locked/denied can never strand the value for
//! this launch. Mirrors the robust pattern in `commands/publish/identity/auth.rs`,
//! generalized from one hard-coded entry to an arbitrary keyed store.

use crate::sync::MutexExt;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Keychain service name shared by every QuantaMind secret. The per-secret *key*
/// (below) is the keychain "username", so entries never collide.
const SERVICE: &str = "quantamind";

/// Stable keychain keys. Changing one orphans the old entry, so treat as append-only.
pub const VLLM_API_KEY: &str = "vllm-api-key";
pub const SGLANG_API_KEY: &str = "sglang-api-key";

/// Whether a secret reached durable OS storage or only the session copy. `SessionOnly`
/// (keychain locked/denied/absent) means the value works for THIS launch but is not on
/// disk — callers migrating a legacy plaintext secret must NOT delete their only copy
/// unless the write was `Keychain`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Persisted {
    Keychain,
    SessionOnly,
}

/// Process-lifetime fallback map, keyed by the same key used for the keychain entry.
fn mem() -> &'static Mutex<HashMap<String, String>> {
    static MEM: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    MEM.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Store `secret` under `key`. ALWAYS keep a session copy first (so a denied keychain
/// prompt can never strand it), then make a best-effort durable write. Never panics.
pub fn store(key: &str, secret: &str) -> Persisted {
    mem().lock_recover().insert(key.to_string(), secret.to_string());
    match keyring::Entry::new(SERVICE, key).and_then(|e| e.set_password(secret)) {
        Ok(()) => Persisted::Keychain,
        Err(_) => Persisted::SessionOnly,
    }
}

/// The stored secret, or `None` if never set. Prefers the in-memory session copy (so we
/// never re-prompt the keychain once we have it this launch); only a cold session reads
/// the secure store, caching a hit into memory.
pub fn get(key: &str) -> Option<String> {
    if let Some(v) = mem().lock_recover().get(key).cloned() {
        return Some(v);
    }
    match keyring::Entry::new(SERVICE, key).and_then(|e| e.get_password()) {
        Ok(v) => {
            mem().lock_recover().insert(key.to_string(), v.clone());
            Some(v)
        }
        Err(_) => None,
    }
}

/// Forget a secret on both stores — best-effort, never panics.
pub fn clear(key: &str) {
    if let Ok(e) = keyring::Entry::new(SERVICE, key) {
        let _ = e.delete_credential();
    }
    mem().lock_recover().remove(key);
}

/// Keychain key for one MCP server's env-var VALUE. Namespaced by server id so
/// two servers using the same var name never collide. Only the var *name* is
/// ever written to `mcp_servers.yaml`; the value lives here (rule 7a).
pub fn mcp_env_key(server_id: &str, env_var: &str) -> String {
    format!("mcp-env.{server_id}.{env_var}")
}

#[cfg(test)]
#[path = "secrets_tests.rs"]
mod tests;
