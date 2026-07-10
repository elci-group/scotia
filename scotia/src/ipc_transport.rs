use crate::ipc::{DaemonRequest, DaemonResponse};
use anyhow::{Context, Result};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// Hard cap on a single IPC message (request or response). Control-plane
/// payloads are tiny; anything larger is treated as hostile or malformed so a
/// peer cannot exhaust daemon memory (CWE-400 / CWE-770).
pub const MAX_IPC_MESSAGE_BYTES: usize = 64 * 1024; // 64 KiB

/// Per-read timeout so a stalled peer cannot pin a handler task forever.
const IPC_IO_TIMEOUT: Duration = Duration::from_secs(30);

/// Read one newline-terminated, UTF-8 message from `stream`, bounded in both
/// size and time. Returns `Ok(None)` only on a clean EOF before any byte is
/// read; an EOF after a partial message is an error.
async fn read_message(stream: &mut UnixStream) -> Result<Option<String>> {
    let mut raw: Vec<u8> = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = match tokio::time::timeout(IPC_IO_TIMEOUT, stream.read(&mut byte)).await {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(e).context("failed to read IPC message"),
            Err(_) => anyhow::bail!("timed out reading IPC message"),
        };
        if n == 0 {
            if raw.is_empty() {
                return Ok(None);
            }
            anyhow::bail!("IPC connection closed mid-message");
        }
        if raw.len() >= MAX_IPC_MESSAGE_BYTES {
            anyhow::bail!(
                "IPC message exceeds maximum size of {} bytes",
                MAX_IPC_MESSAGE_BYTES
            );
        }
        raw.push(byte[0]);
        if byte[0] == b'\n' {
            break;
        }
    }
    let line = String::from_utf8(raw).context("IPC message was not valid UTF-8")?;
    Ok(Some(line))
}

/// Send a request and read a response over a Unix stream.
/// Write one request to a Unix stream without waiting for a response.
///
/// Used for fire-and-forget frames such as the handshake `Auth` (which the
/// server acknowledges implicitly by keeping the connection open) and by
/// [`request`] for the normal request/response case.
pub async fn write_request(stream: &mut UnixStream, req: &DaemonRequest) -> Result<()> {
    let line = serde_json::to_string(req).context("failed to serialize IPC request")?;
    stream
        .write_all(line.as_bytes())
        .await
        .context("failed to write IPC request")?;
    stream
        .write_all(b"\n")
        .await
        .context("failed to write newline")?;
    stream
        .flush()
        .await
        .context("failed to flush IPC request")?;
    Ok(())
}

pub async fn request(stream: &mut UnixStream, req: DaemonRequest) -> Result<DaemonResponse> {
    write_request(stream, &req).await?;

    let Some(line) = read_message(stream).await? else {
        anyhow::bail!("daemon closed the connection without a response");
    };
    let resp: DaemonResponse =
        serde_json::from_str(&line).context("failed to deserialize IPC response")?;
    Ok(resp)
}

/// Read one request from a Unix stream.
pub async fn read_request(stream: &mut UnixStream) -> Result<Option<DaemonRequest>> {
    let Some(line) = read_message(stream).await? else {
        return Ok(None);
    };
    let req: DaemonRequest =
        serde_json::from_str(&line).context("failed to deserialize IPC request")?;
    Ok(Some(req))
}

/// Write a response to a Unix stream.
pub async fn write_response(stream: &mut UnixStream, resp: DaemonResponse) -> Result<()> {
    let line = serde_json::to_string(&resp).context("failed to serialize IPC response")?;
    stream
        .write_all(line.as_bytes())
        .await
        .context("failed to write IPC response")?;
    stream.write_all(b"\n").await?;
    stream
        .flush()
        .await
        .context("failed to flush IPC response")?;
    Ok(())
}

/// Try to connect to the daemon socket. Returns None if the daemon is not running.
pub async fn try_connect(socket_path: &std::path::Path) -> Option<UnixStream> {
    UnixStream::connect(socket_path).await.ok()
}

/// Connect to the daemon, transparently performing the optional handshake-token
/// authentication when a token file is present next to the socket.
///
/// If `<socket-dir>/token` exists and is readable, its contents are sent as an
/// `Auth` request before the stream is returned; if the daemon rejects it, the
/// connection is dropped (returns `None`). When no token file is present this
/// behaves exactly like [`try_connect`], so callers are unaffected when the
/// daemon runs without `--require-token`.
pub async fn try_connect_authed(socket_path: &std::path::Path) -> Option<UnixStream> {
    let mut stream = UnixStream::connect(socket_path).await.ok()?;
    if let Some(parent) = socket_path.parent() {
        let token_path = parent.join("token");
        if let Ok(token) = tokio::fs::read_to_string(&token_path).await {
            let token = token.trim().to_string();
            if !token.is_empty() {
                let resp = request(&mut stream, DaemonRequest::Auth { token })
                    .await
                    .ok()?;
                if !matches!(resp, DaemonResponse::Ok) {
                    return None;
                }
            }
        }
    }
    Some(stream)
}

/// Convenience: register a run with the daemon, ignoring failures.
pub async fn register_run(
    socket_path: &std::path::Path,
    run_id: uuid::Uuid,
    agent: crate::event::AgentKind,
    task: Option<String>,
    cwd: std::path::PathBuf,
) {
    let Some(mut stream) = try_connect_authed(socket_path).await else {
        return;
    };
    let req = crate::ipc::DaemonRequest::RegisterRun {
        run_id,
        agent,
        task,
        cwd,
        started_at: chrono::Utc::now(),
    };
    let _ = request(&mut stream, req).await;
}

/// Convenience: finish a run with the daemon, ignoring failures.
pub async fn finish_run(
    socket_path: &std::path::Path,
    run_id: uuid::Uuid,
    exit_code: Option<i32>,
    actions: usize,
    models: usize,
    errors: usize,
    retries: usize,
) {
    let Some(mut stream) = try_connect_authed(socket_path).await else {
        return;
    };
    let req = crate::ipc::DaemonRequest::FinishRun {
        run_id,
        exit_code,
        actions,
        models,
        errors,
        retries,
        finished_at: chrono::Utc::now(),
    };
    let _ = request(&mut stream, req).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::AgentKind;
    use crate::ipc::DaemonRequest;
    use tokio::net::UnixListener;

    #[tokio::test]
    async fn request_roundtrip_over_unix_socket() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let req = read_request(&mut stream).await.unwrap().unwrap();
            let resp = match req {
                DaemonRequest::Ping => crate::ipc::DaemonResponse::Pong,
                _ => crate::ipc::DaemonResponse::Error {
                    message: "expected ping".to_string(),
                },
            };
            write_response(&mut stream, resp).await.unwrap();
        });

        let mut client = UnixStream::connect(&socket_path).await.unwrap();
        let resp = request(&mut client, DaemonRequest::Ping).await.unwrap();

        server.await.unwrap();
        assert!(matches!(resp, crate::ipc::DaemonResponse::Pong));
    }

    #[tokio::test]
    async fn try_connect_returns_none_when_no_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("missing.sock");
        assert!(try_connect(&socket_path).await.is_none());
    }

    #[tokio::test]
    async fn register_run_ignores_missing_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("missing.sock");
        register_run(
            &socket_path,
            uuid::Uuid::new_v4(),
            AgentKind::KimiCode,
            None,
            std::path::PathBuf::from("."),
        )
        .await;
        // Should not panic or block.
    }

    #[tokio::test]
    async fn read_request_rejects_oversize_message() {
        use tokio::io::AsyncWriteExt;
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("oversize.sock");
        let listener = tokio::net::UnixListener::bind(&socket_path).unwrap();

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream).await
        });

        let mut client = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
        // One byte over the limit, with no newline terminator.
        let payload = vec![b'A'; MAX_IPC_MESSAGE_BYTES + 1];
        client.write_all(&payload).await.unwrap();
        drop(client);

        let result = server.await.unwrap();
        assert!(result.is_err(), "oversize IPC message must be rejected");
    }

    /// Minimal server that optionally requires a handshake token, then answers
    /// a single Ping with Pong.
    async fn run_token_server(
        listener: tokio::net::UnixListener,
        expected: Option<String>,
        pong: tokio::sync::oneshot::Sender<()>,
    ) {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        if let Some(exp) = expected {
            match read_request(&mut stream).await {
                Ok(Some(DaemonRequest::Auth { token })) if token == exp => {
                    let _ = write_response(&mut stream, DaemonResponse::Ok).await;
                }
                _ => return, // reject: drop the connection
            }
        }
        // Expect a Ping and answer Pong.
        if let Ok(Some(DaemonRequest::Ping)) = read_request(&mut stream).await {
            let _ = write_response(&mut stream, DaemonResponse::Pong).await;
            let _ = pong.send(());
        }
    }

    #[tokio::test]
    async fn authed_connect_succeeds_with_correct_token() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("auth.sock");
        std::fs::write(dir.path().join("token"), "secret").unwrap();
        let listener = tokio::net::UnixListener::bind(&socket_path).unwrap();

        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(run_token_server(listener, Some("secret".to_string()), tx));

        let mut stream = try_connect_authed(&socket_path).await.expect("connect ok");
        let resp = request(&mut stream, DaemonRequest::Ping).await.unwrap();
        assert!(matches!(resp, DaemonResponse::Pong));
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), rx).await;
    }

    #[tokio::test]
    async fn authed_connect_fails_with_wrong_token() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("auth.sock");
        std::fs::write(dir.path().join("token"), "wrong").unwrap();
        let listener = tokio::net::UnixListener::bind(&socket_path).unwrap();

        let (tx, _rx) = tokio::sync::oneshot::channel();
        tokio::spawn(run_token_server(listener, Some("secret".to_string()), tx));

        assert!(try_connect_authed(&socket_path).await.is_none());
    }

    #[tokio::test]
    async fn authed_connect_works_without_token_file() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("plain.sock");
        // No token file present -> behaves like a plain connect.
        let listener = tokio::net::UnixListener::bind(&socket_path).unwrap();

        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(run_token_server(listener, None, tx));

        let mut stream = try_connect_authed(&socket_path).await.expect("connect ok");
        let resp = request(&mut stream, DaemonRequest::Ping).await.unwrap();
        assert!(matches!(resp, DaemonResponse::Pong));
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), rx).await;
    }
}
