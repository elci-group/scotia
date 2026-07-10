//! Daemon startup setup: bind and secure the control socket, and write the
//! optional handshake token and PID file with owner-only permissions.

use anyhow::{Context, Result};
use scotia::ipc::default_token_path;
use scotia::runtime::{ensure_private_dir, set_owner_only};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::net::UnixListener;
use tracing::info;

/// Remove any stale socket, ensure its parent directory is private (0700), bind
/// a fresh listener, and lock the socket node to owner-only (0600).
pub async fn prepare_socket(socket_path: &Path) -> Result<UnixListener> {
    // `remove_file` unlinks the named entry itself and does not follow a symlink
    // to a target, so this cannot delete an attacker-chosen victim file.
    if socket_path.exists() {
        tokio::fs::remove_file(socket_path)
            .await
            .with_context(|| format!("failed to remove stale socket {}", socket_path.display()))?;
    }

    if let Some(parent) = socket_path.parent() {
        ensure_private_dir(parent)
            .with_context(|| format!("failed to create socket directory {}", parent.display()))?;
    }

    let listener = UnixListener::bind(socket_path)
        .with_context(|| format!("failed to bind daemon socket {}", socket_path.display()))?;

    // Combined with the 0700 parent directory this prevents any other local user
    // from connecting.
    set_owner_only(socket_path)
        .with_context(|| format!("failed to secure socket {}", socket_path.display()))?;

    Ok(listener)
}

/// When `require` is set, generate a fresh handshake token, write it owner-only
/// next to the socket, and return it for the accept loop to enforce.
pub async fn write_token(require: bool) -> Result<Option<Arc<str>>> {
    if !require {
        return Ok(None);
    }
    let token_path = default_token_path();
    if let Some(parent) = token_path.parent() {
        ensure_private_dir(parent).ok();
    }
    let token = uuid::Uuid::new_v4().simple().to_string();
    tokio::fs::write(&token_path, &token)
        .await
        .with_context(|| format!("failed to write IPC token {}", token_path.display()))?;
    set_owner_only(&token_path)
        .with_context(|| format!("failed to secure IPC token {}", token_path.display()))?;
    info!("scotiad IPC token enabled at {}", token_path.display());
    Ok(Some(Arc::from(token.as_str())))
}

/// Write the daemon PID file when a path was requested.
pub async fn write_pid(pid_file: Option<&PathBuf>) -> Result<()> {
    let Some(pid_file) = pid_file else {
        return Ok(());
    };
    if let Some(parent) = pid_file.parent() {
        ensure_private_dir(parent).ok();
    }
    let pid = std::process::id().to_string();
    tokio::fs::write(pid_file, pid)
        .await
        .with_context(|| format!("failed to write PID file {}", pid_file.display()))?;
    Ok(())
}
