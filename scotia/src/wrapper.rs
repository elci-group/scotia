//! Spawn an agent process, tee its stdio through Scotia, and capture the run.
//!
//! The bounded line reader and stdio pumps live in [`io`]; this module holds the
//! public [`WrapperConfig`] and the [`run_and_capture`] orchestrator.

mod io;

use crate::event::{AgentKind, ScotiaEvent, ScotiaRun};
use crate::interceptor::{AgentInterceptor, InterceptorContext, StreamSource, build_interceptor};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::BufReader;
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

    let mut child = spawn_agent(&config)?;

    let stdout = child.stdout.take().context("agent stdout not available")?;
    let stderr = child.stderr.take().context("agent stderr not available")?;
    let stdin = child.stdin.take().context("agent stdin not available")?;

    let stdout_handle = tokio::spawn(io::pipe_output(
        BufReader::new(stdout),
        tokio::io::stdout(),
        interceptor.clone(),
        events.clone(),
        ctx.clone(),
        StreamSource::Stdout,
    ));
    let stderr_handle = tokio::spawn(io::pipe_output(
        BufReader::new(stderr),
        tokio::io::stderr(),
        interceptor.clone(),
        events.clone(),
        ctx.clone(),
        StreamSource::Stderr,
    ));
    let stdin_handle = tokio::spawn(io::pipe_input(tokio::io::stdin(), stdin));

    let exit_status = child.wait().await.context("failed to wait on agent")?;

    // Wait for streams to drain before finalizing so no lines are lost.
    let _ = stdout_handle.await;
    let _ = stderr_handle.await;
    let _ = stdin_handle.await;

    let exit_code = exit_status.code();
    let all_events = collect_events(&interceptor, &ctx, events, exit_code).await?;
    let all_events = ensure_run_started(all_events, &run, &config, run_id);

    let mut run = run;
    run.events = all_events;
    run.finish(exit_code, None);
    Ok(run)
}

/// Build and spawn the agent child process with piped stdio.
fn spawn_agent(config: &WrapperConfig) -> Result<tokio::process::Child> {
    let mut cmd = Command::new(&config.program);
    cmd.args(&config.args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    if let Some(dir) = &config.working_dir {
        cmd.current_dir(dir);
    }

    cmd.spawn().with_context(|| {
        format!(
            "failed to spawn agent {} with program {}",
            config.agent.as_str(),
            config.program
        )
    })
}

/// Drain the interceptor's final events and unwrap the shared event vec.
async fn collect_events(
    interceptor: &SharedInterceptor,
    ctx: &InterceptorContext,
    events: Arc<Mutex<Vec<ScotiaEvent>>>,
    exit_code: Option<i32>,
) -> Result<Vec<ScotiaEvent>> {
    let mut final_events = {
        let mut interceptor = interceptor.lock().await;
        interceptor.finalize(ctx, exit_code)
    };

    {
        let mut events_guard = events.lock().await;
        events_guard.append(&mut final_events);
    }

    Arc::try_unwrap(events)
        .map_err(|_| anyhow::anyhow!("event arc still held"))
        .map(|m| m.into_inner())
}

/// Ensure the run starts with `RunStarted`; the captured stream may not include
/// it because the wrapper synthesizes it when creating the `ScotiaRun`.
fn ensure_run_started(
    mut events: Vec<ScotiaEvent>,
    run: &ScotiaRun,
    config: &WrapperConfig,
    run_id: uuid::Uuid,
) -> Vec<ScotiaEvent> {
    if !events
        .iter()
        .any(|e| matches!(e, ScotiaEvent::RunStarted { .. }))
    {
        events.insert(
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
    events
}
