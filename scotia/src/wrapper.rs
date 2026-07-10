use crate::event::{AgentKind, ScotiaEvent, ScotiaRun};
use crate::interceptor::{AgentInterceptor, InterceptorContext, StreamSource, build_interceptor};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;

/// Configuration for wrapping an agent process.
#[derive(Debug, Clone)]
pub struct WrapperConfig {
    pub agent: AgentKind,
    pub task: Option<String>,
    pub program: String,
    pub args: Vec<String>,
    pub working_dir: Option<std::path::PathBuf>,
    pub run_id: Option<uuid::Uuid>,
}

pub type SharedInterceptor = Arc<Mutex<Box<dyn AgentInterceptor>>>;

/// Hard cap on the number of bytes a single logical line may accumulate before
/// the wrapper splits it. A hostile or runaway agent that emits a multi-gigabyte
/// newline-free stream must not be able to make the wrapper allocate an
/// unbounded buffer; lines beyond this cap are emitted in `MAX_LINE_BYTES`
/// fragments (bounded memory, lossy-but-bounded).
pub(crate) const MAX_LINE_BYTES: usize = 1024 * 1024;

/// Read one line with a hard per-line byte cap.
///
/// Returns `Ok(None)` at EOF. A line longer than [`MAX_LINE_BYTES`] is yielded
/// across successive calls in cap-sized fragments; the trailing `\n` (when
/// present) is stripped. Fragmentation is bounded in memory and never reads more
/// than `MAX_LINE_BYTES` ahead of the consumer.
pub(crate) async fn read_line_bounded<R>(
    reader: &mut BufReader<R>,
) -> std::io::Result<Option<String>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut out: Vec<u8> = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(if out.is_empty() {
                None
            } else {
                Some(String::from_utf8_lossy(&out).into_owned())
            });
        }

        let room = MAX_LINE_BYTES - out.len();
        let scan_len = available.len().min(room);
        match available[..scan_len].iter().position(|&b| b == b'\n') {
            Some(pos) => {
                out.extend_from_slice(&available[..pos]);
                reader.consume(pos + 1); // drop the newline
                return Ok(Some(String::from_utf8_lossy(&out).into_owned()));
            }
            None => {
                out.extend_from_slice(&available[..scan_len]);
                reader.consume(scan_len);
                if out.len() == MAX_LINE_BYTES {
                    // Cap reached: emit this fragment; the remainder will be
                    // returned on the next call(s).
                    return Ok(Some(String::from_utf8_lossy(&out).into_owned()));
                }
            }
        }
    }
}

/// Spawn the agent, tee its stdio through Scotia, and return the captured run.
pub async fn run_and_capture(config: WrapperConfig) -> Result<ScotiaRun> {
    let run = ScotiaRun::new(config.agent, config.task.clone(), config.run_id);
    let run_id = run.run_id;
    let ctx = InterceptorContext {
        run_id,
        agent: config.agent,
        hints: HashMap::new(),
    };
    let interceptor: SharedInterceptor = Arc::new(Mutex::new(build_interceptor(config.agent)));
    let events: Arc<Mutex<Vec<ScotiaEvent>>> = Arc::new(Mutex::new(Vec::new()));

    let mut cmd = Command::new(&config.program);
    cmd.args(&config.args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    if let Some(dir) = &config.working_dir {
        cmd.current_dir(dir);
        // Hint the working directory to the interceptor for side-channel log discovery.
        let mut ctx = ctx.clone();
        ctx.hints
            .insert("cwd".to_string(), dir.display().to_string());
    }

    let mut child = cmd.spawn().with_context(|| {
        format!(
            "failed to spawn agent {} with program {}",
            config.agent.as_str(),
            config.program
        )
    })?;

    let stdout = child.stdout.take().context("agent stdout not available")?;
    let stderr = child.stderr.take().context("agent stderr not available")?;
    let stdin = child.stdin.take().context("agent stdin not available")?;

    let stdout_handle = tokio::spawn(pipe_output(
        BufReader::new(stdout),
        tokio::io::stdout(),
        interceptor.clone(),
        events.clone(),
        ctx.clone(),
        StreamSource::Stdout,
    ));
    let stderr_handle = tokio::spawn(pipe_output(
        BufReader::new(stderr),
        tokio::io::stderr(),
        interceptor.clone(),
        events.clone(),
        ctx.clone(),
        StreamSource::Stderr,
    ));
    let stdin_handle = tokio::spawn(pipe_input(tokio::io::stdin(), stdin));

    let exit_status = child.wait().await.context("failed to wait on agent")?;

    // Wait for streams to drain before finalizing so no lines are lost.
    let _ = stdout_handle.await;
    let _ = stderr_handle.await;
    let _ = stdin_handle.await;

    let exit_code = exit_status.code();

    let mut final_events = {
        let mut interceptor = interceptor.lock().await;
        interceptor.finalize(&ctx, exit_code)
    };

    {
        let mut events_guard = events.lock().await;
        events_guard.append(&mut final_events);
    }

    let mut all_events = Arc::try_unwrap(events)
        .map_err(|_| anyhow::anyhow!("event arc still held"))?
        .into_inner();

    // Ensure the run always starts with RunStarted; the captured stream may not
    // include it because the wrapper synthesizes it when creating ScotiaRun.
    if !all_events
        .iter()
        .any(|e| matches!(e, ScotiaEvent::RunStarted { .. }))
    {
        all_events.insert(
            0,
            ScotiaEvent::RunStarted {
                run_id,
                agent: config.agent,
                task: config.task.clone(),
                timestamp: run.started_at,
                metadata: Default::default(),
            },
        );
    }

    let mut run = run;
    run.events = all_events;
    run.finish(exit_code, None);
    Ok(run)
}

async fn pipe_output<R, W>(
    mut reader: BufReader<R>,
    mut writer: W,
    interceptor: SharedInterceptor,
    events: Arc<Mutex<Vec<ScotiaEvent>>>,
    ctx: InterceptorContext,
    source: StreamSource,
) where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut warned_overlong = false;
    loop {
        let line = match read_line_bounded(&mut reader).await {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(e) => {
                tracing::warn!("failed to read {} line: {}", source_label(source), e);
                break;
            }
        };

        if !warned_overlong && line.len() == MAX_LINE_BYTES {
            tracing::warn!(
                "{} emitted a line longer than {} bytes; splitting into bounded fragments",
                source_label(source),
                MAX_LINE_BYTES
            );
            warned_overlong = true;
        }

        let parsed = {
            let mut interceptor = interceptor.lock().await;
            interceptor.parse_line(&ctx, source, &line)
        };
        {
            let mut events_guard = events.lock().await;
            events_guard.extend(parsed);
        }

        if let Err(e) = writer.write_all(line.as_bytes()).await {
            tracing::warn!("failed to write {} line: {}", source_label(source), e);
            break;
        }
        if let Err(e) = writer.write_all(b"\n").await {
            tracing::warn!("failed to write newline: {}", e);
            break;
        }
        if let Err(e) = writer.flush().await {
            tracing::warn!("failed to flush {}: {}", source_label(source), e);
            break;
        }
    }
}

async fn pipe_input<R, W>(reader: R, mut writer: W)
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {
                if writer.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
                if writer.flush().await.is_err() {
                    break;
                }
            }
            Err(e) => {
                tracing::warn!("failed to read stdin: {}", e);
                break;
            }
        }
    }
}

fn source_label(source: StreamSource) -> &'static str {
    match source {
        StreamSource::Stdout => "stdout",
        StreamSource::Stderr => "stderr",
        StreamSource::SideChannel => "side_channel",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn read_line_bounded_reads_lines_and_eof() {
        let data = b"hello\nworld".to_vec();
        let mut reader = BufReader::new(&data[..]);
        assert_eq!(
            read_line_bounded(&mut reader).await.unwrap().as_deref(),
            Some("hello")
        );
        assert_eq!(
            read_line_bounded(&mut reader).await.unwrap().as_deref(),
            Some("world")
        );
        assert_eq!(read_line_bounded(&mut reader).await.unwrap(), None);
    }

    #[tokio::test]
    async fn read_line_bounded_handles_last_line_without_newline() {
        let data = b"only-line".to_vec();
        let mut reader = BufReader::new(&data[..]);
        assert_eq!(
            read_line_bounded(&mut reader).await.unwrap().as_deref(),
            Some("only-line")
        );
        assert_eq!(read_line_bounded(&mut reader).await.unwrap(), None);
    }

    #[tokio::test]
    async fn read_line_bounded_splits_overlong_lines() {
        let mut data = vec![b'a'; MAX_LINE_BYTES + 10];
        data.push(b'\n');
        let mut reader = BufReader::new(&data[..]);
        let first = read_line_bounded(&mut reader).await.unwrap().unwrap();
        assert_eq!(first.len(), MAX_LINE_BYTES);
        assert!(first.bytes().all(|b| b == b'a'));
        let second = read_line_bounded(&mut reader).await.unwrap().unwrap();
        assert_eq!(second.len(), 10);
        assert!(second.bytes().all(|b| b == b'a'));
        assert_eq!(read_line_bounded(&mut reader).await.unwrap(), None);
    }
}
