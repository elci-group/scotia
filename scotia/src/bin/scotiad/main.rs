mod serve;
mod setup;

use anyhow::{Context, Result};
use clap::Parser;
use scotia::daemon::Daemon;
use scotia::ipc::{default_socket_path, default_token_path};
use scotia::notify::{Notifier, daemon_started, default_notifier};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tracing::info;

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

    /// Require clients to present the handshake token (written mode 0600 next to
    /// the socket) before any other request. Defence-in-depth for shared or
    /// containerised hosts; defaults to off because the socket is already
    /// restricted to the owning user.
    #[arg(long, default_value_t = false)]
    require_token: bool,
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

    let listener = setup::prepare_socket(&socket_path).await?;
    let expected_token = setup::write_token(args.require_token).await?;
    setup::write_pid(args.pid_file.as_ref()).await?;

    info!("scotiad listening on {}", socket_path.display());

    let notifier: Arc<dyn Notifier> = default_notifier();
    notifier
        .notify(daemon_started())
        .context("failed to emit daemon-started notification")?;

    let daemon = Daemon::new(notifier, args.progress_interval);
    let shutdown = tokio::sync::broadcast::channel(1).0;

    let (accept_handle, ticker_handle) = serve::start(
        listener,
        daemon,
        expected_token.clone(),
        Duration::from_secs(args.progress_interval),
        chrono::Duration::seconds(args.prune_after_seconds),
        shutdown.clone(),
    );

    // Wait for shutdown signal.
    let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())?;
    tokio::select! {
        _ = signal::ctrl_c() => info!("received SIGINT, shutting down"),
        _ = sigterm.recv() => info!("received SIGTERM, shutting down"),
    }

    let _ = shutdown.send(());
    accept_handle.await?;
    ticker_handle.await?;

    if socket_path.exists() {
        tokio::fs::remove_file(&socket_path).await.ok();
    }
    if expected_token.is_some() {
        tokio::fs::remove_file(default_token_path()).await.ok();
    }
    if let Some(pid_file) = &args.pid_file {
        tokio::fs::remove_file(pid_file).await.ok();
    }

    Ok(())
}
