use anyhow::{Context, Result};
use clap::Parser;
use scotia::daemon::Daemon;
use scotia::ipc::{DaemonRequest, DaemonResponse, default_socket_path};
use scotia::ipc_transport::{read_request, write_response};
use scotia::notify::{Notifier, daemon_started, default_notifier};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UnixListener;
use tokio::signal;
use tracing::{info, warn};

#[derive(Debug, clap::Parser)]
#[command(name = "scotiad")]
#[command(about = "Scotia daemon — session registry and notification hub")]
struct Args {
    /// Path to the daemon socket.
    #[arg(long, default_value = None)]
    socket: Option<PathBuf>,

    /// Path to write the daemon PID.
    #[arg(long, default_value = None)]
    pid_file: Option<PathBuf>,

    /// Seconds between progress-notification ticks.
    #[arg(long, default_value_t = 60)]
    progress_interval: u64,

    /// How long to keep finished runs in memory before pruning.
    #[arg(long, default_value_t = 3600)]
    prune_after_seconds: i64,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let socket_path = args.socket.unwrap_or_else(default_socket_path);

    // Remove stale socket file if it exists.
    if socket_path.exists() {
        tokio::fs::remove_file(&socket_path)
            .await
            .with_context(|| format!("failed to remove stale socket {}", socket_path.display()))?;
    }

    // Ensure parent directory exists.
    if let Some(parent) = socket_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create socket directory {}", parent.display()))?;
    }

    let listener = UnixListener::bind(&socket_path)
        .with_context(|| format!("failed to bind daemon socket {}", socket_path.display()))?;

    // Write PID file if requested.
    if let Some(pid_file) = &args.pid_file {
        if let Some(parent) = pid_file.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        let pid = std::process::id().to_string();
        tokio::fs::write(pid_file, pid)
            .await
            .with_context(|| format!("failed to write PID file {}", pid_file.display()))?;
    }

    info!("scotiad listening on {}", socket_path.display());

    let notifier: Arc<dyn Notifier> = default_notifier();
    notifier.notify(daemon_started()).context("failed to emit daemon-started notification")?;

    let daemon = Daemon::new(notifier, args.progress_interval);
    let shutdown = tokio::sync::broadcast::channel(1).0;

    // Progress notification ticker.
    let ticker_daemon = daemon.clone();
    let mut ticker_shutdown = shutdown.subscribe();
    let progress_interval = Duration::from_secs(args.progress_interval);
    let prune_duration = chrono::Duration::seconds(args.prune_after_seconds);
    let ticker_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(progress_interval);
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = ticker_daemon.tick_progress_notifications().await {
                        warn!("progress notification tick failed: {}", e);
                    }
                    ticker_daemon.prune(prune_duration).await;
                }
                _ = ticker_shutdown.recv() => break,
            }
        }
    });

    // Accept loop.
    let accept_daemon = daemon.clone();
    let accept_shutdown = shutdown.clone();
    let mut accept_shutdown_rx = accept_shutdown.subscribe();
    let accept_handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((mut stream, _)) => {
                            let d = accept_daemon.clone();
                            tokio::spawn(async move {
                                if let Err(e) = handle_client(&mut stream, d).await {
                                    warn!("client handler error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            warn!("accept error: {}", e);
                        }
                    }
                }
                _ = accept_shutdown_rx.recv() => break,
            }
        }
    });

    // Wait for shutdown signal.
    let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())?;
    tokio::select! {
        _ = signal::ctrl_c() => info!("received SIGINT, shutting down"),
        _ = sigterm.recv() => {
            info!("received SIGTERM, shutting down");
        }
    }

    let _ = shutdown.send(());
    accept_handle.await?;
    ticker_handle.await?;

    if socket_path.exists() {
        tokio::fs::remove_file(&socket_path).await.ok();
    }
    if let Some(pid_file) = &args.pid_file {
        tokio::fs::remove_file(pid_file).await.ok();
    }

    Ok(())
}

async fn handle_client(stream: &mut tokio::net::UnixStream, daemon: Daemon) -> Result<()> {
    while let Some(req) = read_request(stream).await? {
        let resp = match req {
            DaemonRequest::Ping => DaemonResponse::Pong,
            DaemonRequest::RegisterRun {
                run_id,
                agent,
                task,
                cwd,
                started_at: _,
            } => {
                match daemon.register_run(run_id, agent, task, cwd).await {
                    Ok(()) => DaemonResponse::Ok,
                    Err(e) => DaemonResponse::Error {
                        message: format!("register_run failed: {}", e),
                    },
                }
            }
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
