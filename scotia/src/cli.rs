mod daemon;
mod doctor;
mod query;
mod run;
mod shims;

use crate::event::AgentKind;
use crate::notify::{Notifier, default_notifier};
use crate::storage::StorageConfig;
use crate::tui::run_tui;
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

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

        /// Absolute path to the agent binary; bypasses PATH resolution. When
        /// set, the positional arguments after `--` become the agent's args.
        #[arg(long)]
        agent_path: Option<PathBuf>,

        /// Do not resolve the agent via PATH: require --agent-path, a pinned
        /// path in agents.json, or an absolute program path.
        #[arg(long, default_value_t = false)]
        no_path_fallback: bool,

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

    /// Diagnose the Scotia installation, runtime layout, shims, and daemon.
    Doctor,

    /// Apply an installation (used by GUI installers).
    Installer {
        #[command(subcommand)]
        command: InstallerCommands,
    },
}

#[derive(Subcommand)]
enum InstallerCommands {
    /// Apply the installation with the chosen scope.
    Apply {
        /// Installation scope.
        #[arg(long, value_enum, default_value_t = crate::installer::InstallScope::User)]
        scope: crate::installer::InstallScope,

        /// Start the daemon automatically.
        #[arg(long, default_value_t = true)]
        autostart: bool,

        /// Install PATH shims for agent binaries.
        #[arg(long, default_value_t = true)]
        install_shims: bool,

        /// Directory containing the Scotia binaries.
        #[arg(long, default_value = None)]
        bin_dir: Option<PathBuf>,
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
    /// Install the daemon as a user service (systemd/launchd).
    InstallService,
    /// Uninstall the daemon user service.
    UninstallService,
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
            agent_path,
            no_path_fallback,
            command,
        }) => {
            run::run_command(
                agent,
                task,
                cwd,
                agent_path,
                no_path_fallback,
                command,
                &storage_config,
                &notifier,
            )
            .await?;
        }
        Some(Commands::Replay { path }) => {
            query::replay(&path).await?;
        }
        Some(Commands::Summary { path }) => {
            query::summary(&path).await?;
        }
        Some(Commands::List) => {
            query::list(&storage_config).await?;
        }
        Some(Commands::Validate { path }) => {
            query::validate_run(&path).await?;
        }
        Some(Commands::Diff { left, right }) => {
            query::diff(&left, &right).await?;
        }
        Some(Commands::Regression { path }) => {
            query::regression(&path).await?;
        }
        Some(Commands::InstallShims { shim_dir }) => {
            shims::install(shim_dir, &notifier)?;
        }
        Some(Commands::UninstallShims { shim_dir }) => {
            shims::uninstall(shim_dir, &notifier)?;
        }
        Some(Commands::Notify { command }) => match command {
            None | Some(NotifyCommands::Test) => {
                for n in [
                    crate::notify::daemon_started(),
                    crate::notify::run_started(
                        AgentKind::KimiCode,
                        &std::env::current_dir()?,
                        None,
                    ),
                    crate::notify::run_finished(AgentKind::KimiCode, 12, 3, 0, 0),
                    crate::notify::run_finished(AgentKind::Codex, 12, 3, 2, 1),
                    crate::notify::run_crashed(AgentKind::ClaudeCode, Some(1)),
                ] {
                    notifier.notify(n)?;
                }
            }
        },
        Some(Commands::Installer { command }) => match command {
            InstallerCommands::Apply {
                scope,
                autostart,
                install_shims,
                bin_dir,
            } => {
                let options = crate::installer::InstallOptions {
                    scope,
                    autostart,
                    install_shims,
                    bin_dir: bin_dir.unwrap_or_else(crate::installer::default_bin_dir),
                };
                crate::installer::apply_install(&options)?;
                println!(
                    "Scotia installed ({scope} scope)",
                    scope = options.scope.as_str()
                );
            }
        },
        Some(Commands::Daemon { command }) => {
            daemon::handle_daemon_command(command).await?;
        }
        Some(Commands::Doctor) => {
            doctor::run_doctor().await?;
        }
    }

    Ok(())
}
