use anyhow::{Context, Result};
use clap::Parser;
use scotia::daemon::{Daemon, handle_client};
use scotia::ipc::{default_socket_path, default_token_path};
use scotia::notify::{Notifier, daemon_started, default_notifier};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UnixListener;
use tokio::signal;
use tokio::sync::Semaphore;
use tracing::{info, warn};

/// Maximum simultaneous client connections handled by the daemon. Beyond this,
/// new connections are dropped rather than allowed to exhaust the runtime.
const MAX_CONCURRENT_CONNECTIONS: usize = 64;

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

/// Create a directory (recursively) and lock it down to owner-only access.
///
/// Even if the directory already existed we re-assert the mode, so a
/// previously-loose directory cannot be reused to expose the socket.
fn create_private_dir(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(path)
    }
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

    // Remove stale socket file if it exists. `remove_file` unlinks the named
    // entry itself (it does not follow a symlink to a target), so this cannot
    // be used to delete an attacker-chosen victim file.
    if socket_path.exists() {
        tokio::fs::remove_file(&socket_path)
            .await
            .with_context(|| format!("failed to remove stale socket {}", socket_path.display()))?;
    }

    // Ensure the socket's parent directory exists and is private to us (0700).
    if let Some(parent) = socket_path.parent() {
        create_private_dir(parent)
            .with_context(|| format!("failed to create socket directory {}", parent.display()))?;
    }

    let listener = UnixListener::bind(&socket_path)
        .with_context(|| format!("failed to bind daemon socket {}", socket_path.display()))?;

    // Lock the socket node itself to owner-only (0600). Combined with the 0700
    // parent directory this prevents any other local user from connecting.
    #[cfg(unix)]
    std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to secure socket {}", socket_path.display()))?;

    // Optional handshake token. Generated fresh each start, written owner-only
    // next to the socket; only the same user can read it.
    let expected_token: Option<Arc<str>> = if args.require_token {
        let token_path = default_token_path();
        if let Some(parent) = token_path.parent() {
            create_private_dir(parent).ok();
        }
        let token = uuid::Uuid::new_v4().simple().to_string();
        tokio::fs::write(&token_path, &token)
            .await
            .with_context(|| format!("failed to write IPC token {}", token_path.display()))?;
        #[cfg(unix)]
        std::fs::set_permissions(&token_path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to secure IPC token {}", token_path.display()))?;
        info!("scotiad IPC token enabled at {}", token_path.display());
        Some(Arc::from(token.as_str()))
    } else {
        None
    };

    // Write PID file if requested.
    if let Some(pid_file) = &args.pid_file {
        if let Some(parent) = pid_file.parent() {
            create_private_dir(parent).ok();
        }
        let pid = std::process::id().to_string();
        tokio::fs::write(pid_file, pid)
            .await
            .with_context(|| format!("failed to write PID file {}", pid_file.display()))?;
    }

    info!("scotiad listening on {}", socket_path.display());

    let notifier: Arc<dyn Notifier> = default_notifier();
    notifier
        .notify(daemon_started())
        .context("failed to emit daemon-started notification")?;

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
    let conn_limit = Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS));
    // Move a clone into the accept task; keep the original for the shutdown cleanup check.
    let accept_token = expected_token.clone();
    let accept_handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((mut stream, _)) => {
                            let d = accept_daemon.clone();
                            let t = accept_token.clone();
                            // Shed load once the concurrency cap is reached
                            // instead of queueing unbounded tasks.
                            match conn_limit.clone().try_acquire_owned() {
                                Ok(permit) => {
                                    tokio::spawn(async move {
                                        let _permit = permit;
                                        if let Err(e) = handle_client(&mut stream, d, t).await {
                                            warn!("client handler error: {}", e);
                                        }
                                    });
                                }
                                Err(_) => {
                                    warn!("connection limit reached; dropping client");
                                }
                            }
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
    if expected_token.is_some() {
        tokio::fs::remove_file(default_token_path()).await.ok();
    }
    if let Some(pid_file) = &args.pid_file {
        tokio::fs::remove_file(pid_file).await.ok();
    }

    Ok(())
}
