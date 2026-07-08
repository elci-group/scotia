use crate::ipc::{DaemonRequest, DaemonResponse};
use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Send a request and read a response over a Unix stream.
pub async fn request(stream: &mut UnixStream, req: DaemonRequest) -> Result<DaemonResponse> {
    let line = serde_json::to_string(&req).context("failed to serialize IPC request")?;
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

    let mut reader = BufReader::new(stream);
    let mut buf = String::new();
    reader
        .read_line(&mut buf)
        .await
        .context("failed to read IPC response")?;
    let resp: DaemonResponse =
        serde_json::from_str(&buf).context("failed to deserialize IPC response")?;
    Ok(resp)
}

/// Read one request from a Unix stream.
pub async fn read_request(stream: &mut UnixStream) -> Result<Option<DaemonRequest>> {
    let mut reader = BufReader::new(stream);
    let mut buf = String::new();
    let n = reader
        .read_line(&mut buf)
        .await
        .context("failed to read IPC request")?;
    if n == 0 {
        return Ok(None);
    }
    let req: DaemonRequest =
        serde_json::from_str(&buf).context("failed to deserialize IPC request")?;
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

/// Convenience: register a run with the daemon, ignoring failures.
pub async fn register_run(
    socket_path: &std::path::Path,
    run_id: uuid::Uuid,
    agent: crate::event::AgentKind,
    task: Option<String>,
    cwd: std::path::PathBuf,
) {
    let Some(mut stream) = try_connect(socket_path).await else {
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
    let Some(mut stream) = try_connect(socket_path).await else {
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
}
