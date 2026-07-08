use super::auth::{store_refresh_token, Persisted};
use super::pkce::{await_redirect, new_state, pkce_pair};
use super::token::exchange_code;
use crate::commands::publish::auth_state::AuthState;
use crate::commands::publish::publish_cmd::publish_api;
use crate::errors::AppError;
use crate::inference::http::http::probe_client;
use std::time::Duration;
use tauri::{AppHandle, State};
use tauri_plugin_shell::ShellExt;
use tokio::net::TcpListener;

/// How long to wait for the user to complete the browser sign-in before giving up.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

/// How long to wait for the publish server to answer the pre-flight probe.
const REACH_TIMEOUT: Duration = Duration::from_secs(5);

/// Confirm the publish server answers HTTP before we open the browser and arm the
/// 300s loopback wait. ANY HTTP response (even 4xx) means reachable; a transport
/// error (refused/unroutable) means it's down. Stops a stopped `:8787` from hanging
/// sign-in for five minutes — the user gets an immediate, accurate message instead.
async fn ensure_reachable(base: &str) -> Result<(), AppError> {
    let probe = probe_client()?.get(format!("{base}/authorize")).send();
    match tokio::time::timeout(REACH_TIMEOUT, probe).await {
        Ok(Ok(_)) => Ok(()),
        _ => Err(AppError::Validation(format!(
            "Can't reach the publish server at {base} — is it running?"
        ))),
    }
}

/// PKCE browser sign-in: mint a verifier/challenge, bind an ephemeral loopback
/// listener, open the system browser to `/authorize`, catch the redirect code,
/// exchange it for tokens, and store the rotated refresh token + cache the access
/// token. Thin glue over the unit-tested pkce/redirect/exchange pieces. A timeout
/// keeps a never-completed sign-in from hanging the listener forever. Returns whether
/// the refresh token reached durable keychain storage (`false` = session-only, so the
/// UI can warn the user they may need to sign in again next launch).
// `Shell::open` is deprecated in favor of tauri-plugin-opener; we stay on the
// already-present shell plugin (dep-light) — revisit if we adopt opener elsewhere.
#[allow(deprecated)]
#[tauri::command]
pub async fn start_login(app: AppHandle, state: State<'_, AuthState>) -> Result<bool, AppError> {
    ensure_reachable(publish_api()).await?;

    let (verifier, challenge) = pkce_pair();
    let oauth_state = new_state();
    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|e| AppError::Io(e.to_string()))?;
    let port = listener.local_addr().map_err(|e| AppError::Io(e.to_string()))?.port();
    let redirect = format!("http://127.0.0.1:{port}/callback");

    let url = reqwest::Url::parse_with_params(
        &format!("{}/authorize", publish_api()),
        &[
            ("response_type", "code"),
            ("scope", "publish"),
            ("code_challenge_method", "S256"),
            ("code_challenge", &challenge),
            ("redirect_uri", &redirect),
            ("state", &oauth_state),
        ],
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;

    app.shell().open(url.to_string(), None).map_err(|e| AppError::Internal(e.to_string()))?;

    let code = tokio::time::timeout(LOGIN_TIMEOUT, await_redirect(listener, &oauth_state))
        .await
        .map_err(|_| AppError::Validation("sign-in timed out — please try again".into()))??;

    let tokens = exchange_code(publish_api(), &code, &verifier).await?;
    let persisted = store_refresh_token(&tokens.refresh_token);
    state.set(tokens.access_token);
    Ok(persisted == Persisted::Keychain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    /// A down server (nothing listening on the port) fails fast with a clear message,
    /// not the 300s sign-in timeout.
    #[tokio::test]
    async fn unreachable_server_errors_immediately() {
        // Bind then drop to obtain a port guaranteed to refuse connections.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let base = format!("http://127.0.0.1:{port}");

        let err = ensure_reachable(&base).await.unwrap_err();
        match err {
            AppError::Validation(msg) => assert!(
                msg.contains("Can't reach the publish server"),
                "unexpected message: {msg}"
            ),
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    /// Any HTTP response (here a bare 200) counts as reachable.
    /// `#[cfg(unix)]`-gated: the single-shot mock (accept once, respond, drop)
    /// races the client under Windows's socket-close semantics — the connection
    /// resets before `ensure_reachable`'s HTTP client finishes reading the
    /// response, producing a spurious red on windows-latest CI. The reachability
    /// probe itself is portable (used everywhere in the app on all three OSes);
    /// only this specific mock-server test is Unix-loopback-flaky.
    #[cfg(unix)]
    #[tokio::test]
    async fn responding_server_is_reachable() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").await;
            }
        });
        let base = format!("http://127.0.0.1:{port}");

        assert!(ensure_reachable(&base).await.is_ok());
    }
}
