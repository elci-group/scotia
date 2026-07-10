//! `scotia daemon ...` — control the per-user scotiad.
//!
//! Extracted from `cli.rs`; behavior is identical. Only the dispatcher in
//! `cli.rs` calls `handle_daemon_command`.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::ipc::{
    DaemonRequest, DaemonResponse, default_log_file, default_pid_file, default_socket_path,
};
use crate::ipc_transport::{request, try_connect_authed};

pub(super) async fn handle_daemon_command(command: super::DaemonCommands) -> Result<()> {
    let socket_path = default_socket_path();
    let pid_file = default_pid_file();
    let log_file = default_log_file();

    match command {
        super::DaemonCommands::Start => {
            if try_connect_authed(&socket_path).await.is_some() {
                println!("scotiad is already running");
                return Ok(());
            }
            if let Some(parent) = log_file.parent() {
                tokio::fs::create_dir_all(parent).await.ok();
            }
            let scotiad = std::env::current_exe()?
                .parent()
                .map(|p| p.join("scotiad"))
                .filter(|p| crate::shim::is_safe_executable(p))
                .or_else(|| {
                    std::env::var_os("PATH").and_then(|pv| {
                        std::env::split_paths(&pv)
                            .map(|d| d.join("scotiad"))
                            .find(|c| crate::shim::is_safe_executable(c))
                    })
                })
                .unwrap_or_else(|| PathBuf::from("scotiad"));

            let log = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_file)
                .with_context(|| format!("failed to open daemon log {}", log_file.display()))?;

            let mut cmd = tokio::process::Command::new(scotiad);
            cmd.arg("--socket")
                .arg(&socket_path)
                .arg("--pid-file")
                .arg(&pid_file)
                .stdout(std::process::Stdio::from(log.try_clone()?))
                .stderr(std::process::Stdio::from(log))
                .kill_on_drop(false);

            let child = cmd.spawn().context("failed to spawn scotiad")?;
            println!("Started scotiad (PID {})", child.id().unwrap_or(0));
        }
        super::DaemonCommands::Stop => {
            let pid: Option<i32> = if pid_file.exists() {
                tokio::fs::read_to_string(&pid_file)
                    .await
                    .ok()
                    .and_then(|s| s.trim().parse().ok())
            } else {
                None
            };

            if let Some(pid) = pid {
                std::process::Command::new("kill")
                    .arg("-TERM")
                    .arg(pid.to_string())
                    .status()
                    .context("failed to send SIGTERM to scotiad")?;
                println!("Sent SIGTERM to scotiad (PID {})", pid);
            } else {
                println!("No PID file found; scotiad may not be running");
            }
        }
        super::DaemonCommands::Status => {
            let mut stream = match try_connect_authed(&socket_path).await {
                Some(s) => s,
                None => {
                    println!("scotiad is not running");
                    return Ok(());
                }
            };
            let resp = request(&mut stream, DaemonRequest::Ping).await?;
            match resp {
                DaemonResponse::Pong => println!("scotiad is running"),
                _ => println!("unexpected response from scotiad"),
            }

            let resp = request(&mut stream, DaemonRequest::ListRuns).await?;
            if let DaemonResponse::Runs { runs } = resp {
                let active = runs.iter().filter(|r| r.is_active()).count();
                println!("Active runs: {}", active);
                println!("Recent runs:");
                for run in runs.iter().take(10) {
                    let status = if run.is_active() {
                        "active".to_string()
                    } else {
                        format!("finished (exit {})", run.exit_code.unwrap_or(-1))
                    };
                    println!(
                        "  {} — {} — {} — {}",
                        run.run_id.to_string().split('-').next().unwrap_or("?"),
                        run.agent.as_str(),
                        status,
                        format_duration(run.duration().to_std().unwrap_or_default())
                    );
                }
            }
        }
        super::DaemonCommands::Logs => {
            if log_file.exists() {
                let contents = tokio::fs::read_to_string(&log_file).await?;
                print!("{}", contents);
            } else {
                println!("No daemon log found at {}", log_file.display());
            }
        }
        super::DaemonCommands::InstallService => {
            let result = crate::service::install_service()?;
            println!("Installed {} service", result.platform.as_str());
            if let Some(path) = result.installed_path {
                println!("  -> {}", path.display());
            }
            if !result.output.is_empty() {
                println!("{}", result.output);
            }
        }
        super::DaemonCommands::UninstallService => {
            let result = crate::service::uninstall_service()?;
            println!("Uninstalled {} service", result.platform.as_str());
            if !result.output.is_empty() {
                println!("{}", result.output);
            }
        }
    }

    Ok(())
}

fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}
