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
    stream.write_all(b"\n").await.context("failed to write newline")?;
    stream.flush().await.context("failed to flush IPC request")?;

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
    stream.flush().await.context("failed to flush IPC response")?;
    Ok(())
}

/// Try to connect to the daemon socket. Returns None if the daemon is not running.
pub async fn try_connect(socket_path: &std::path::Path) -> Option<UnixStream> {
    match UnixStream::connect(socket_path).await {
        Ok(s) => Some(s),
        Err(_) => None,
    }
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
