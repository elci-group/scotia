use crate::algebra::{diff_runs, regression_suite, render_regression_suite, validate};
use crate::event::AgentKind;
use crate::storage::{StorageConfig, list_runs, load_run, store_run};
use crate::wrapper::{WrapperConfig, run_and_capture};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "scotia")]
#[command(about = "Semantic Decision Ledger for agentic systems")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Root directory for Scotia logs.
    #[arg(long, global = true, default_value = "scotia-log")]
    log_root: PathBuf,

    /// Commit each artifact to the surrounding Git repository.
    #[arg(long, global = true)]
    git_commit: bool,
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

    match cli.command {
        Commands::Run {
            agent,
            task,
            cwd,
            command,
        } => {
            let program = command.first().cloned().context("no command provided")?;
            let args = command.into_iter().skip(1).collect();
            let agent_kind = AgentKind::from_binary_name(&agent);

            let config = WrapperConfig {
                agent: agent_kind,
                task,
                program,
                args,
                working_dir: cwd,
            };

            let run = run_and_capture(config).await?;
            let stored = store_run(&storage_config, run).await?;

            println!("Scotia captured run {}", stored.run_id);
            println!("  JSON:    {}", stored.json_path.display());
            println!("  Summary: {}", stored.summary_path.display());
            println!("  Graph:   {}", stored.dot_path.display());
        }
        Commands::Replay { path } => {
            let run = load_run(&path).await?;
            for event in run.events {
                println!("{}", serde_json::to_string(&event)?);
            }
        }
        Commands::Summary { path } => {
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
        Commands::List => {
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
        Commands::Validate { path } => {
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
        Commands::Diff { left, right } => {
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
        Commands::Regression { path } => {
            let run = load_run(&path).await?;
            let suite = regression_suite(&run);
            println!("{}", render_regression_suite(&suite));
        }
    }

    Ok(())
}
