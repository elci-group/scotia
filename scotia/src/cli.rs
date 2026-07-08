use crate::algebra::{diff_runs, regression_suite, render_regression_suite, validate};
use crate::event::{AgentKind, ScotiaEvent};
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
    let notifier: Box<dyn Notifier> = if cli.no_notify {
        Box::new(crate::notify::TerminalNotifier)
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

            notifier.notify(run_started(
                agent_kind,
                &working_dir,
                task.as_deref(),
            ))?;

            let config = WrapperConfig {
                agent: agent_kind,
                task: task.clone(),
                program,
                args,
                working_dir: cwd,
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
            notifier.notify(finish_note)?;

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
    }

    Ok(())
}
