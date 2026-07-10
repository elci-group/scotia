//! `scotia doctor` — environment and installation health check.
//!
//! Verifies the daemon runtime layout (private socket dir, owner-only socket
//! and token), that the shim directory is on `PATH` and actually resolves for
//! each agent name (to a safe executable), and that a running daemon can be
//! reached with the handshake token. Prints a `[ok]`/`[warn]`/`[fail]` report
//! and returns an error (non-zero exit) when any check fails hard.

use crate::ipc::{default_socket_path, default_token_path, scotia_base_dir};
use crate::ipc_transport::try_connect_authed;
use crate::shim::{
    DEFAULT_AGENT_NAMES, default_shim_dir, find_in_path, is_safe_executable, path_entries,
};
use anyhow::Result;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Ok,
    Warn,
    Fail,
}

impl CheckStatus {
    fn label(self) -> &'static str {
        match self {
            CheckStatus::Ok => "ok",
            CheckStatus::Warn => "warn",
            CheckStatus::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Check {
    pub status: CheckStatus,
    pub name: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShimResolution {
    /// Our shim is the first match on PATH and points at a safe executable.
    Active,
    /// A different binary earlier on PATH shadows our shim.
    Shadowed { resolved: PathBuf },
    /// The shim exists but its target is not a safe executable.
    UnsafeTarget { target: PathBuf },
    /// No binary of this name is found anywhere on PATH.
    NotFound,
}

/// Classify what `name` resolves to on `entries` relative to our `shim_dir`.
/// Pure over the provided PATH entries so it is unit-testable with temp dirs.
pub fn classify_shim(name: &str, shim_dir: &Path, entries: &[PathBuf]) -> ShimResolution {
    let Some(resolved) = find_in_path(name, entries) else {
        return ShimResolution::NotFound;
    };
    let shim_dir_canon = std::fs::canonicalize(shim_dir).unwrap_or_else(|_| shim_dir.to_path_buf());
    let resolved_canon = std::fs::canonicalize(&resolved).unwrap_or_else(|_| resolved.clone());

    let is_ours = resolved_canon.parent() == Some(&shim_dir_canon)
        || resolved.parent() == Some(shim_dir)
        || resolved.parent() == Some(&shim_dir_canon);

    if !is_ours {
        return ShimResolution::Shadowed { resolved };
    }

    // It is our shim; verify the executable it ultimately runs is safe.
    let target = resolved_canon;
    if is_safe_executable(&target) {
        ShimResolution::Active
    } else {
        ShimResolution::UnsafeTarget { target }
    }
}

#[cfg(unix)]
fn dir_private(mode: u32) -> bool {
    mode & 0o777 == 0o700
}

#[cfg(unix)]
fn file_owner_only(mode: u32) -> bool {
    mode & 0o777 == 0o600
}

#[cfg(unix)]
fn mode_string(mode: u32) -> String {
    format!("{:04o}", mode & 0o777)
}

pub async fn run_doctor() -> Result<Vec<Check>> {
    let mut checks = Vec::new();

    check_runtime_dir(&mut checks);
    check_socket(&mut checks);
    check_token(&mut checks);
    check_shims(&mut checks);
    check_daemon_reachable(&mut checks).await;

    for c in &checks {
        println!("[{}] {}: {}", c.status.label(), c.name, c.detail);
    }

    let failures = checks
        .iter()
        .filter(|c| c.status == CheckStatus::Fail)
        .count();
    if failures > 0 {
        anyhow::bail!("doctor found {failures} failing check(s)");
    }
    Ok(checks)
}

fn check_runtime_dir(checks: &mut Vec<Check>) {
    let dir = scotia_base_dir();
    if !dir.exists() {
        checks.push(Check {
            status: CheckStatus::Warn,
            name: "runtime dir",
            detail: format!("{} does not exist yet (daemon not started)", dir.display()),
        });
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&dir) {
            Ok(m) => {
                let mode = m.mode();
                if dir_private(mode) {
                    checks.push(Check {
                        status: CheckStatus::Ok,
                        name: "runtime dir",
                        detail: format!("{} mode {}", dir.display(), mode_string(mode)),
                    });
                } else {
                    checks.push(Check {
                        status: CheckStatus::Fail,
                        name: "runtime dir",
                        detail: format!(
                            "{} mode {} (expected 0700, owner-only)",
                            dir.display(),
                            mode_string(mode)
                        ),
                    });
                }
            }
            Err(e) => checks.push(Check {
                status: CheckStatus::Fail,
                name: "runtime dir",
                detail: format!("cannot stat {}: {e}", dir.display()),
            }),
        }
    }
    #[cfg(not(unix))]
    checks.push(Check {
        status: CheckStatus::Ok,
        name: "runtime dir",
        detail: format!("{} exists", dir.display()),
    });
}

fn check_socket(checks: &mut Vec<Check>) {
    let sock = default_socket_path();
    if !sock.exists() {
        checks.push(Check {
            status: CheckStatus::Warn,
            name: "socket",
            detail: format!("{} not present (daemon not running)", sock.display()),
        });
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&sock) {
            Ok(m) => {
                let mode = m.mode();
                if file_owner_only(mode) {
                    checks.push(Check {
                        status: CheckStatus::Ok,
                        name: "socket",
                        detail: format!("{} mode {}", sock.display(), mode_string(mode)),
                    });
                } else {
                    checks.push(Check {
                        status: CheckStatus::Fail,
                        name: "socket",
                        detail: format!(
                            "{} mode {} (expected 0600, owner-only)",
                            sock.display(),
                            mode_string(mode)
                        ),
                    });
                }
            }
            Err(e) => checks.push(Check {
                status: CheckStatus::Fail,
                name: "socket",
                detail: format!("cannot stat {}: {e}", sock.display()),
            }),
        }
    }
    #[cfg(not(unix))]
    checks.push(Check {
        status: CheckStatus::Ok,
        name: "socket",
        detail: format!("{} exists", sock.display()),
    });
}

fn check_token(checks: &mut Vec<Check>) {
    let token = default_token_path();
    if !token.exists() {
        checks.push(Check {
            status: CheckStatus::Warn,
            name: "ipc token",
            detail: format!(
                "{} absent (token auth not enabled; ok unless daemon uses --require-token)",
                token.display()
            ),
        });
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&token) {
            Ok(m) => {
                let mode = m.mode();
                let len = m.len();
                if file_owner_only(mode) && len > 0 {
                    checks.push(Check {
                        status: CheckStatus::Ok,
                        name: "ipc token",
                        detail: format!(
                            "{} mode {}, {len} bytes",
                            token.display(),
                            mode_string(mode)
                        ),
                    });
                } else {
                    checks.push(Check {
                        status: CheckStatus::Fail,
                        name: "ipc token",
                        detail: format!(
                            "{} mode {} (expected 0600), {len} bytes",
                            token.display(),
                            mode_string(mode)
                        ),
                    });
                }
            }
            Err(e) => checks.push(Check {
                status: CheckStatus::Fail,
                name: "ipc token",
                detail: format!("cannot stat {}: {e}", token.display()),
            }),
        }
    }
    #[cfg(not(unix))]
    checks.push(Check {
        status: CheckStatus::Ok,
        name: "ipc token",
        detail: format!("{} exists", token.display()),
    });
}

fn check_shims(checks: &mut Vec<Check>) {
    let shim_dir = default_shim_dir();
    let entries = path_entries();
    let shim_dir_canon = std::fs::canonicalize(&shim_dir).unwrap_or_else(|_| shim_dir.clone());
    let on_path = entries
        .iter()
        .any(|e| std::fs::canonicalize(e).unwrap_or_else(|_| e.clone()) == shim_dir_canon);

    checks.push(if on_path {
        Check {
            status: CheckStatus::Ok,
            name: "shim dir on PATH",
            detail: format!("{} is on PATH", shim_dir.display()),
        }
    } else {
        Check {
            status: CheckStatus::Warn,
            name: "shim dir on PATH",
            detail: format!(
                "{} is not on PATH (run `scotia install-shims` or re-source your shell)",
                shim_dir.display()
            ),
        }
    });

    for name in DEFAULT_AGENT_NAMES {
        match classify_shim(name, &shim_dir, &entries) {
            ShimResolution::Active => checks.push(Check {
                status: CheckStatus::Ok,
                name: "shim",
                detail: format!("{name} -> active shim"),
            }),
            ShimResolution::Shadowed { resolved } => checks.push(Check {
                status: CheckStatus::Warn,
                name: "shim",
                detail: format!(
                    "{name} resolves to {} (shadows any Scotia shim)",
                    resolved.display()
                ),
            }),
            ShimResolution::UnsafeTarget { target } => checks.push(Check {
                status: CheckStatus::Fail,
                name: "shim",
                detail: format!(
                    "{name} shim target {} is not a safe executable",
                    target.display()
                ),
            }),
            ShimResolution::NotFound => checks.push(Check {
                status: CheckStatus::Warn,
                name: "shim",
                detail: format!("{name} not found on PATH (not shimmed)"),
            }),
        }
    }
}

async fn check_daemon_reachable(checks: &mut Vec<Check>) {
    let sock = default_socket_path();
    match try_connect_authed(&sock).await {
        Some(_stream) => checks.push(Check {
            status: CheckStatus::Ok,
            name: "daemon",
            detail: format!("reachable (authed) at {}", sock.display()),
        }),
        None => checks.push(Check {
            status: CheckStatus::Warn,
            name: "daemon",
            detail: format!(
                "not reachable at {} (start with `scotia daemon start`)",
                sock.display()
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn dir_private_accepts_0700_only() {
        assert!(dir_private(0o40700));
        assert!(!dir_private(0o40755));
        assert!(!dir_private(0o40750));
        assert!(!dir_private(0o40777));
    }

    #[cfg(unix)]
    #[test]
    fn file_owner_only_accepts_0600_only() {
        assert!(file_owner_only(0o100600));
        assert!(!file_owner_only(0o100644));
        assert!(!file_owner_only(0o100666));
        assert!(!file_owner_only(0o100700));
    }

    #[test]
    fn classify_shim_reports_not_found_when_absent() {
        let dir = std::env::temp_dir().join("scotia-doctor-empty");
        let entries: Vec<PathBuf> = vec![];
        assert_eq!(
            classify_shim("codex", &dir, &entries),
            ShimResolution::NotFound
        );
    }

    #[cfg(unix)]
    #[test]
    fn classify_shim_detects_shadowing() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = std::env::temp_dir().join(format!("scotia-doctor-{}", uuid::Uuid::new_v4()));
        let real_dir = tmp.join("realbin");
        let shim_dir = tmp.join("shims");
        std::fs::create_dir_all(&real_dir).unwrap();
        std::fs::create_dir_all(&shim_dir).unwrap();

        // A real `codex` earlier on PATH than our shim dir shadows the shim.
        let real = real_dir.join("codex");
        std::fs::write(&real, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o755)).unwrap();

        let entries = vec![real_dir.clone(), shim_dir.clone()];
        match classify_shim("codex", &shim_dir, &entries) {
            ShimResolution::Shadowed { resolved } => assert_eq!(resolved, real),
            other => panic!("expected Shadowed, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
