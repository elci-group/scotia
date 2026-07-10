//! Daemon serving tasks: the progress/prune ticker and the client accept loop.

use scotia::daemon::{Daemon, handle_client};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UnixListener;
use tokio::sync::{Semaphore, broadcast};
use tokio::task::JoinHandle;
use tracing::warn;

/// Maximum simultaneous client connections handled by the daemon. Beyond this,
/// new connections are dropped rather than allowed to exhaust the runtime.
const MAX_CONCURRENT_CONNECTIONS: usize = 64;

/// Spawn the progress/prune ticker and the client accept loop, returning their
/// join handles. Both stop when a message is sent on `shutdown`.
pub fn start(
    listener: UnixListener,
    daemon: Daemon,
    token: Option<Arc<str>>,
    progress_interval: Duration,
    prune_duration: chrono::Duration,
    shutdown: broadcast::Sender<()>,
) -> (JoinHandle<()>, JoinHandle<()>) {
    let ticker = spawn_ticker(
        daemon.clone(),
        shutdown.subscribe(),
        progress_interval,
        prune_duration,
    );
    let accept = spawn_accept_loop(listener, daemon, token, shutdown.subscribe());
    (accept, ticker)
}

fn spawn_ticker(
    daemon: Daemon,
    mut shutdown: broadcast::Receiver<()>,
    progress_interval: Duration,
    prune_duration: chrono::Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(progress_interval);
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = daemon.tick_progress_notifications().await {
                        warn!("progress notification tick failed: {}", e);
                    }
                    daemon.prune(prune_duration).await;
                }
                _ = shutdown.recv() => break,
            }
        }
    })
}

fn spawn_accept_loop(
    listener: UnixListener,
    daemon: Daemon,
    token: Option<Arc<str>>,
    mut shutdown: broadcast::Receiver<()>,
) -> JoinHandle<()> {
    let conn_limit = Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS));
    tokio::spawn(async move {
        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((mut stream, _)) => {
                            let d = daemon.clone();
                            let t = token.clone();
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
                                Err(_) => warn!("connection limit reached; dropping client"),
                            }
                        }
                        Err(e) => warn!("accept error: {}", e),
                    }
                }
                _ = shutdown.recv() => break,
            }
        }
    })
}
