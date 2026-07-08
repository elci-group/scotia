use crate::algebra::{diff_runs, regression_suite, render_regression_suite, validate};
use crate::event::{AgentKind, ScotiaEvent};
use crate::ipc::{DaemonRequest, DaemonResponse, default_log_file, default_pid_file, default_socket_path};
use crate::ipc_transport::{request, try_connect, register_run as daemon_register_run, finish_run as daemon_finish_run};
use crate::notify::{Notifier, default_notifier, run_crashed, run_finished, run_started};
use crate::shim::{
    DEFAULT_AGENT_NAMES, default_shim_dir, detect_aliases, find_scotia_shim_binary,
    install_shims, remove_shell_path, uninstall_shims, update_shell_path,
};
use crate::storage::{StorageConfig, list_runs, load_run, store_run};
use crate::tui::run_tui;
use crate::wrapper::{WrapperConfig, run_and_capture};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "scotia")]
#[command(about = "Semantic Decision Ledger for agentic systems")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Root directory for Scotia logs.
    #[arg(long, global = true, default_value = "scotia-log")]
    log_root: PathBuf,

    /// Commit each artifact to the surrounding Git repository.
    #[arg(long, global = true)]
    git_commit: bool,

    /// Disable desktop notifications for this invocation.
    #[arg(long, global = true)]
    no_notify: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Wrap and observe an agent process.
    Run {
        /// Agent kind (kimi-code, agy, cosine, codex, claude-code, opencode).
        #[arg(short, long)]
        agent: String,

        /// Optional task description.
        #[arg(short, long)]
        task: Option<String>,

        /// Working directory for the agent.
        #[arg(short, long)]
        cwd: Option<PathBuf>,

        /// Program and arguments for the agent.
        #[arg(required = true, num_args = 1..)]
        command: Vec<String>,
    },

    /// Replay a stored run to stdout.
    Replay {
        /// Path to the run JSON file.
        path: PathBuf,
    },

    /// Print the summary for a stored run.
    Summary {
        /// Path to the run JSON file.
        path: PathBuf,
    },

    /// List stored runs.
    List,

    /// Validate the structural correctness of a stored run.
    Validate {
        /// Path to the run JSON file.
        path: PathBuf,
    },

    /// Diff two stored runs.
    Diff {
        /// Path to the left run JSON file.
        left: PathBuf,
        /// Path to the right run JSON file.
        right: PathBuf,
    },

    /// Generate a regression assertion suite from a stored run.
    Regression {
        /// Path to the run JSON file.
        path: PathBuf,
    },

    /// Install shims so agent commands are auto-wrapped.
    InstallShims {
        /// Directory where shims are created.
        #[arg(long)]
        shim_dir: Option<PathBuf>,
    },

    /// Remove Scotia shims from PATH.
    UninstallShims {
        /// Directory where shims were created.
        #[arg(long)]
        shim_dir: Option<PathBuf>,
    },

    /// Test the notification system.
    Notify {
        #[command(subcommand)]
        command: Option<NotifyCommands>,
    },

    /// Control the Scotia daemon.
    Daemon {
        #[command(subcommand)]
        command: DaemonCommands,
    },
}

#[derive(Subcommand)]
enum DaemonCommands {
    /// Start the daemon in the background.
    Start,
    /// Stop the running daemon.
    Stop,
    /// Show daemon status and recent runs.
    Status,
    /// Tail the daemon log.
    Logs,
}

#[derive(Subcommand)]
enum NotifyCommands {
    /// Send a test notification for each severity level.
    Test,
}

pub async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();
    let storage_config = StorageConfig {
        root: cli.log_root,
        commit_to_git: cli.git_commit,
    };
    let notifier: Arc<dyn Notifier> = if cli.no_notify {
        Arc::new(crate::notify::TerminalNotifier)
    } else {
        default_notifier()
    };

    match cli.command {
        None => {
            run_tui(storage_config).await?;
        }
        Some(Commands::Run {
            agent,
            task,
            cwd,
            command,
        }) => {
            let program = command.first().cloned().context("no command provided")?;
            let args = command.into_iter().skip(1).collect();
            let agent_kind = AgentKind::from_binary_name(&agent);
            let working_dir = cwd
                .clone()
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."));
            let run_id = Uuid::new_v4();
            let socket_path = default_socket_path();

            // Notify locally and register with the daemon (best effort).
            notifier.notify(run_started(
                agent_kind,
                &working_dir,
                task.as_deref(),
            ))?;
            daemon_register_run(
                &socket_path,
                run_id,
                agent_kind,
                task.clone(),
                working_dir.clone(),
            )
            .await;

            let config = WrapperConfig {
                agent: agent_kind,
                task: task.clone(),
                program,
                args,
                working_dir: cwd,
                run_id: Some(run_id),
            };

            let run = run_and_capture(config).await?;

            let actions = run
                .events
                .iter()
                .filter(|e| matches!(e, ScotiaEvent::ActionInvoked { .. }))
                .count();
            let models = run
                .events
                .iter()
                .filter(|e| matches!(e, ScotiaEvent::ModelRouted { .. }))
                .count();
            let errors = run
                .events
                .iter()
                .filter(|e| matches!(e, ScotiaEvent::ErrorOrRetry { .. }))
                .count();
            let retries = run
                .events
                .iter()
                .filter(|e| {
                    matches!(
                        e,
                        ScotiaEvent::ErrorOrRetry {
                            kind: crate::event::ErrorKind::Retry,
                            ..
                        }
                    )
                })
                .count();

            let exit_code = run.events.iter().find_map(|e| match e {
                ScotiaEvent::RunFinished { exit_code, .. } => *exit_code,
                _ => None,
            });

            let finish_note = if exit_code.map(|c| c != 0).unwrap_or(true) && errors > 0 {
                run_crashed(agent_kind, exit_code)
            } else {
                run_finished(agent_kind, actions, models, errors, retries)
            };
            notifier.notify(finish_note.clone())?;
            daemon_finish_run(
                &socket_path,
                run_id,
                exit_code,
                actions,
                models,
                errors,
                retries,
            )
            .await;

            let stored = store_run(&storage_config, run).await?;

            println!("Scotia captured run {}", stored.run_id);
            println!("  JSON:    {}", stored.json_path.display());
            println!("  Summary: {}", stored.summary_path.display());
            println!("  Graph:   {}", stored.dot_path.display());
        }
        Some(Commands::Replay { path }) => {
            let run = load_run(&path).await?;
            for event in run.events {
                println!("{}", serde_json::to_string(&event)?);
            }
        }
        Some(Commands::Summary { path }) => {
            let run = load_run(&path).await?;
            let synthesis = crate::synthesizer::synthesize(&run);
            println!("{}", synthesis.summary);
            if !synthesis.decision_rationales.is_empty() {
                println!("\n## Decision Rationales");
                for r in &synthesis.decision_rationales {
                    println!("- {}", r);
                }
            }
            if !synthesis.trade_offs.is_empty() {
                println!("\n## Trade-offs");
                for t in &synthesis.trade_offs {
                    println!("- {}", t);
                }
            }
        }
        Some(Commands::List) => {
            let runs = list_runs(&storage_config.root).await?;
            if runs.is_empty() {
                println!(
                    "No Scotia runs found under {}",
                    storage_config.root.display()
                );
            } else {
                for run in runs {
                    println!("{}", run.display());
                }
            }
        }
        Some(Commands::Validate { path }) => {
            let run = load_run(&path).await?;
            let issues = validate(&run);
            if issues.is_empty() {
                println!("Run {} is structurally valid.", run.run_id);
            } else {
                println!("Run {} has {} validation issue(s):", run.run_id, issues.len());
                for issue in issues {
                    println!("  - {:?}", issue);
                }
            }
        }
        Some(Commands::Diff { left, right }) => {
            let left_run = load_run(&left).await?;
            let right_run = load_run(&right).await?;
            let diff = diff_runs(&left_run, &right_run);
            println!("Actions added:    {:?}", diff.actions_added);
            println!("Actions removed:  {:?}", diff.actions_removed);
            println!("Models added:     {:?}", diff.models_added);
            println!("Models removed:   {:?}", diff.models_removed);
            println!("Errors added:     {}", diff.errors_added);
            println!("Errors removed:   {}", diff.errors_removed);
        }
        Some(Commands::Regression { path }) => {
            let run = load_run(&path).await?;
            let suite = regression_suite(&run);
            println!("{}", render_regression_suite(&suite));
        }
        Some(Commands::InstallShims { shim_dir }) => {
            let shim_dir = shim_dir.unwrap_or_else(default_shim_dir);
            let scotia_shim = find_scotia_shim_binary()?;
            let aliases = detect_aliases(DEFAULT_AGENT_NAMES);
            if !aliases.is_empty() {
                eprintln!("Detected shell aliases that may shadow shims:");
                for a in &aliases {
                    eprintln!("  - {}", a);
                }
                eprintln!("Consider removing them or re-sourcing your shell config.");
            }
            let result = install_shims(&shim_dir, &scotia_shim, DEFAULT_AGENT_NAMES)?;
            update_shell_path(&shim_dir)?;
            println!("Installed {} shims to {}", result.created.len(), shim_dir.display());
            if !result.collisions.is_empty() {
                eprintln!("Warning: existing binaries earlier in PATH:");
                for c in &result.collisions {
                    eprintln!("  - {}", c);
                }
            }
            notifier.notify(crate::notify::shims_installed(result.created.len()))?;
        }
        Some(Commands::UninstallShims { shim_dir }) => {
            let shim_dir = shim_dir.unwrap_or_else(default_shim_dir);
            let removed = uninstall_shims(&shim_dir, DEFAULT_AGENT_NAMES)?;
            remove_shell_path(&shim_dir)?;
            println!("Removed {} shims from {}", removed.len(), shim_dir.display());
            notifier.notify(crate::notify::shims_uninstalled())?;
        }
        Some(Commands::Notify { command }) => match command {
            None | Some(NotifyCommands::Test) => {
                for n in [
                    crate::notify::daemon_started(),
                    crate::notify::run_started(AgentKind::KimiCode, &std::env::current_dir()?, None),
                    crate::notify::run_finished(AgentKind::KimiCode, 12, 3, 0, 0),
                    crate::notify::run_finished(AgentKind::Codex, 12, 3, 2, 1),
                    crate::notify::run_crashed(AgentKind::ClaudeCode, Some(1)),
                ] {
                    notifier.notify(n)?;
                }
            }
        },
        Some(Commands::Daemon { command }) => {
            handle_daemon_command(command).await?;
        }
    }

    Ok(())
}

async fn handle_daemon_command(command: DaemonCommands) -> Result<()> {
    let socket_path = default_socket_path();
    let pid_file = default_pid_file();
    let log_file = default_log_file();

    match command {
        DaemonCommands::Start => {
            if try_connect(&socket_path).await.is_some() {
                println!("scotiad is already running");
                return Ok(());
            }
            if let Some(parent) = log_file.parent() {
                tokio::fs::create_dir_all(parent).await.ok();
            }
            let scotiad = std::env::current_exe()?
                .parent()
                .map(|p| p.join("scotiad"))
                .filter(|p| p.exists())
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
        DaemonCommands::Stop => {
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
        DaemonCommands::Status => {
            let mut stream = match try_connect(&socket_path).await {
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
        DaemonCommands::Logs => {
            if log_file.exists() {
                let contents = tokio::fs::read_to_string(&log_file).await?;
                print!("{}", contents);
            } else {
                println!("No daemon log found at {}", log_file.display());
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
        format!("{}h {}m {}s", secs / 3600, (secs % 3600) / 60, secs % 60)
    }
}
