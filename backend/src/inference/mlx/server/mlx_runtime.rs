use crate::os::{EngineHost, Host};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// Find a free TCP port in `start..=start+10` by binding then releasing it.
/// Returns `None` if the whole range is taken (never assume a fixed port is
/// free — another service or a stale server may hold it).
pub fn find_available_port(start: u16) -> Option<u16> {
    (start..=start.saturating_add(10)).find(|&port| TcpListener::bind(("127.0.0.1", port)).is_ok())
}

/// Args to launch `mlx_lm.server` for a local model directory on a chosen port.
/// Pure, so it can be asserted without spawning. The model is pre-downloaded to
/// disk (`install_mlx_model`), so this loads it locally — no download step.
pub fn build_spawn_args(model: &str, port: u16) -> Vec<String> {
    vec![
        "--model".into(), model.into(),
        "--host".into(), "127.0.0.1".into(),
        "--port".into(), port.to_string(),
    ]
}

/// Spawn `mlx_lm.server` from `exe`, returning the child so the caller owns its
/// lifecycle. stderr is `piped` so a reader thread can report download/start
/// phase and capture the tail on failure; stdin/stdout are discarded.
pub fn spawn_server(exe: &Path, args: &[String]) -> Result<Child, String> {
    let mut cmd = Command::new(exe);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    Host::apply_spawn_flags(&mut cmd);
    cmd.spawn().map_err(|e| e.to_string())
}

/// Terminate the server. Idempotent: killing an already-exited child is success.
pub fn kill_server(child: &mut Child) -> Result<(), String> {
    match child.kill() {
        Ok(()) => {
            let _ = child.wait();
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_spawn_args_carries_local_model_path_and_chosen_port() {
        let args = build_spawn_args("/Users/me/.quantamind/mlx/mlx-community_X-4bit", 8083);
        assert_eq!(
            args,
            vec!["--model", "/Users/me/.quantamind/mlx/mlx-community_X-4bit", "--host", "127.0.0.1", "--port", "8083"]
        );
    }

    #[test]
    fn find_available_port_returns_an_in_range_port() {
        // Don't re-bind the returned port — that's a TOCTOU race (another
        // process/test can grab it first). The skip behavior is covered below.
        if let Some(port) = find_available_port(8082) {
            assert!((8082..=8092).contains(&port));
        }
    }

    #[test]
    fn find_available_port_skips_an_occupied_port() {
        let occupied = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral");
        let taken = occupied.local_addr().expect("addr").port();
        let found = find_available_port(taken).expect("a free port above the taken one");
        assert_ne!(found, taken);
    }
}
