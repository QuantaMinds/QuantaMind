//! MCP registry commands: CRUD over `mcp_servers.yaml`, per-server secrets to the
//! keychain, and a `probe` that actually connects + lists tools (the "N tools
//! discovered" doctor/preflight moment). Thin — validation/persistence live in
//! `persistence::mcp::servers`, connection in `mcp::client`.

use crate::errors::{AppError, AppResult};
use crate::mcp::client::McpClient;
use crate::mcp::registry::McpServerState;
use crate::persistence::mcp::servers::{load, save, McpServerConfig};
use crate::secrets;
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;
use tauri::Manager;

const MCP_REGISTRY_FILE: &str = "mcp_servers.yaml";
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) fn registry_path(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    let dir = app.path().app_config_dir().map_err(|e| AppError::Io(e.to_string()))?;
    Ok(dir.join(MCP_REGISTRY_FILE))
}

/// Connect to a configured server (env from keychain, roots as trailing args).
/// Shared by `probe` and the Bring-Your-Own run.
pub(crate) async fn connect_configured(cfg: &McpServerConfig) -> AppResult<McpClient> {
    let envs = env_pairs(cfg);
    let args = spawn_args(cfg)?;
    McpClient::connect_with_env(
        &cfg.command,
        &args,
        &envs,
        "quantamind",
        env!("CARGO_PKG_VERSION"),
        PROBE_TIMEOUT,
    )
    .await
}

/// (name, value) env pairs for a server, values pulled from the keychain.
fn env_pairs(cfg: &McpServerConfig) -> Vec<(String, String)> {
    cfg.env_keys
        .iter()
        .filter_map(|k| secrets::get(&secrets::mcp_env_key(&cfg.id, k)).map(|v| (k.clone(), v)))
        .collect()
}

/// Spawn args = configured args + canonical roots as trailing args (how
/// filesystem-style servers take their allowed directories).
fn spawn_args(cfg: &McpServerConfig) -> AppResult<Vec<String>> {
    let mut args = cfg.args.clone();
    for root in cfg.canonical_roots()? {
        args.push(root.to_string_lossy().into_owned());
    }
    Ok(args)
}

#[tauri::command]
pub fn list_mcp_servers(app: tauri::AppHandle) -> Result<Vec<McpServerConfig>, AppError> {
    Ok(load(&registry_path(&app)?)?.servers)
}

/// Add or replace a server config. `save` validates (empty/dup id, empty command).
#[tauri::command]
pub fn upsert_mcp_server(app: tauri::AppHandle, config: McpServerConfig) -> Result<(), AppError> {
    let path = registry_path(&app)?;
    let mut reg = load(&path)?;
    reg.servers.retain(|s| s.id != config.id);
    reg.servers.push(config);
    save(&path, &reg)
}

#[tauri::command]
pub fn remove_mcp_server(
    app: tauri::AppHandle,
    state: tauri::State<'_, McpServerState>,
    id: String,
) -> Result<(), AppError> {
    let path = registry_path(&app)?;
    let mut reg = load(&path)?;
    if let Some(cfg) = reg.get(&id) {
        for k in &cfg.env_keys {
            secrets::clear(&secrets::mcp_env_key(&id, k));
        }
    }
    reg.servers.retain(|s| s.id != id);
    state.remove_and_kill(&id);
    save(&path, &reg)
}

#[tauri::command]
pub fn set_mcp_server_enabled(
    app: tauri::AppHandle,
    id: String,
    enabled: bool,
) -> Result<(), AppError> {
    let path = registry_path(&app)?;
    let mut reg = load(&path)?;
    let s = reg
        .servers
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| AppError::NotFound(format!("mcp server '{id}'")))?;
    s.enabled = enabled;
    save(&path, &reg)
}

/// Store one server env-var value in the keychain (never on disk) and register
/// its name on the config.
#[tauri::command]
pub fn set_mcp_server_secret(
    app: tauri::AppHandle,
    id: String,
    env_var: String,
    value: String,
) -> Result<(), AppError> {
    secrets::store(&secrets::mcp_env_key(&id, &env_var), &value);
    let path = registry_path(&app)?;
    let mut reg = load(&path)?;
    if let Some(s) = reg.servers.iter_mut().find(|s| s.id == id) {
        if !s.env_keys.contains(&env_var) {
            s.env_keys.push(env_var);
        }
    }
    save(&path, &reg)
}

/// What `probe` reports back — the doctor/preflight "N tools discovered".
#[derive(Serialize)]
pub struct McpProbe {
    pub server_name: String,
    pub protocol_version: String,
    pub tool_count: usize,
    pub tool_names: Vec<String>,
}

/// Connect to a configured server, list its tools, and disconnect. Fail-fast +
/// loud: a bad command / stdout-polluting server surfaces here, not mid-run.
#[tauri::command]
pub async fn probe_mcp_server(app: tauri::AppHandle, id: String) -> Result<McpProbe, AppError> {
    let reg = load(&registry_path(&app)?)?;
    let cfg = reg.get(&id).ok_or_else(|| AppError::NotFound(format!("mcp server '{id}'")))?.clone();
    let client = connect_configured(&cfg).await?;
    let result = client.list_tools().await.map(|tools| McpProbe {
        server_name: client.server_info().name.clone(),
        protocol_version: client.protocol_version().to_string(),
        tool_count: tools.tools.len(),
        tool_names: tools.tools.iter().map(|t| t.name.clone()).collect(),
    });
    client.kill();
    result
}
