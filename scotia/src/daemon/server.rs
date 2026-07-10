//! Per-client IPC handler for `scotiad`, lifted out of the binary so the
//! peer-credential and handshake-token gates can be unit-tested with an
//! in-process `UnixStream` pair (no live daemon required).

use std::sync::Arc;

use anyhow::Result;
use tokio::net::UnixStream;
use tracing::warn;

use super::Daemon;
use crate::ipc::{DaemonRequest, DaemonResponse};
use crate::ipc_transport::{read_request, write_response};

/// This process's effective user id (Unix). Used to verify IPC peers.
#[cfg(unix)]
fn current_euid() -> u32 {
    // SAFETY: geteuid() is always safe to call and cannot fail.
    unsafe { libc::geteuid() }
}

/// Handle a single client connection: verify the peer, enforce the optional
/// handshake token, then dispatch requests until the client disconnects.
pub async fn handle_client(
    stream: &mut UnixStream,
    daemon: Daemon,
    expected_token: Option<Arc<str>>,
) -> Result<()> {
    // Reject any peer that is not the same user as this daemon. The socket file
    // permissions are the first line of defence; peer-credential verification is
    // defence in depth so a misconfigured directory cannot become a privilege
    // boundary crossing.
    #[cfg(unix)]
    {
        match stream.peer_cred() {
            Ok(cred) => {
                let expected = current_euid();
                if cred.uid() != expected {
                    warn!(
                        "rejecting IPC client with uid {} (expected {})",
                        cred.uid(),
                        expected
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                warn!("could not read IPC peer credentials ({}); rejecting", e);
                return Ok(());
            }
        }
    }

    // Optional handshake-token gate (default off). When enabled, the client's
    // first frame must be a valid `Auth` request carrying the token.
    if let Some(expected) = &expected_token {
        match read_request(stream).await? {
            Some(DaemonRequest::Auth { token }) if token == expected.as_ref() => {}
            Some(_) => {
                warn!("rejecting IPC client with missing/invalid handshake token");
                return Ok(());
            }
            None => return Ok(()),
        }
    }

    while let Some(req) = read_request(stream).await? {
        let resp = match req {
            // Re-authentication on an already-authenticated connection is a no-op.
            DaemonRequest::Auth { .. } => DaemonResponse::Ok,
            DaemonRequest::Ping => DaemonResponse::Pong,
            DaemonRequest::RegisterRun {
                run_id,
                agent,
                task,
                cwd,
                started_at: _,
            } => match daemon.register_run(run_id, agent, task, cwd).await {
                Ok(()) => DaemonResponse::Ok,
                Err(e) => DaemonResponse::Error {
                    message: format!("register_run failed: {}", e),
                },
            },
            DaemonRequest::FinishRun {
                run_id,
                exit_code,
                actions,
                models,
                errors,
                retries,
                finished_at: _,
            } => match daemon
                .finish_run(run_id, exit_code, actions, models, errors, retries)
                .await
            {
                Ok(()) => DaemonResponse::Ok,
                Err(e) => DaemonResponse::Error {
                    message: format!("finish_run failed: {}", e),
                },
            },
            DaemonRequest::ListRuns => {
                let runs = daemon.list_runs().await;
                DaemonResponse::Runs { runs }
            }
        };
        write_response(stream, resp).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc_transport::{request, write_request};
    use crate::notify::{Notifier, TestNotifier};
    use tokio::net::UnixStream;

    fn test_daemon() -> Daemon {
        let notifier: Arc<dyn Notifier> = Arc::new(TestNotifier::new());
        Daemon::new(notifier, 3600)
    }

    #[tokio::test]
    async fn no_token_required_ping_yields_pong() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let daemon = test_daemon();
        let handle = tokio::spawn(async move { handle_client(&mut server, daemon, None).await });

        let resp = request(&mut client, DaemonRequest::Ping).await.unwrap();
        assert!(matches!(resp, DaemonResponse::Pong));

        drop(client);
        let _ = handle.await;
    }

    #[tokio::test]
    async fn rejects_connection_without_token_when_required() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let daemon = test_daemon();
        let expected: Option<Arc<str>> = Some(Arc::from("secret"));
        let handle =
            tokio::spawn(async move { handle_client(&mut server, daemon, expected).await });

        // First frame is Ping, not the required Auth -> handler closes the socket.
        let res = request(&mut client, DaemonRequest::Ping).await;
        assert!(res.is_err(), "expected rejection without a handshake token");
        let _ = handle.await;
    }

    #[tokio::test]
    async fn rejects_wrong_token_when_required() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let daemon = test_daemon();
        let expected: Option<Arc<str>> = Some(Arc::from("secret"));
        let handle =
            tokio::spawn(async move { handle_client(&mut server, daemon, expected).await });

        let res = request(
            &mut client,
            DaemonRequest::Auth {
                token: "wrong".into(),
            },
        )
        .await;
        assert!(res.is_err(), "expected rejection for a bad token");
        let _ = handle.await;
    }

    #[tokio::test]
    async fn accepts_valid_token_then_serves_requests() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let daemon = test_daemon();
        let expected: Option<Arc<str>> = Some(Arc::from("secret"));
        let handle =
            tokio::spawn(async move { handle_client(&mut server, daemon, expected).await });

        // The handshake Auth is response-less: the server accepts it implicitly
        // by keeping the connection open, so we write it without reading a reply.
        write_request(
            &mut client,
            &DaemonRequest::Auth {
                token: "secret".into(),
            },
        )
        .await
        .unwrap();

        let pong = request(&mut client, DaemonRequest::Ping).await.unwrap();
        assert!(matches!(pong, DaemonResponse::Pong));

        drop(client);
        let _ = handle.await;
    }
}
