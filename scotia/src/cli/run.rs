//! `scotia run` — wrap and observe an agent process.
//!
//! Extracted from `cli.rs` to keep the top-level dispatcher thin. The public
//! surface of `cli` is unchanged (`main` is still the only exported entry).

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use uuid::Uuid;

use crate::event::{AgentKind, ScotiaEvent};
use crate::ipc::default_socket_path;
use crate::ipc_transport::{finish_run as daemon_finish_run, register_run as daemon_register_run};
use crate::notify::{Notifier, run_crashed, run_finished, run_started};
use crate::storage::{StorageConfig, store_run};
use crate::wrapper::{WrapperConfig, run_and_capture};

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_command(
    agent: String,
    task: Option<String>,
    cwd: Option<PathBuf>,
    agent_path: Option<PathBuf>,
    no_path_fallback: bool,
    command: Vec<String>,
    storage_config: &StorageConfig,
    notifier: &Arc<dyn Notifier>,
) -> Result<()> {
    let agent_kind = AgentKind::from_binary_name(&agent);
    let pinned = agent_path.is_some();
    let program = resolve_agent_program(agent_kind, &command, agent_path, no_path_fallback)?;
    let args: Vec<String> = if pinned {
        command
    } else {
        command.into_iter().skip(1).collect()
    };
    let working_dir = cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let run_id = Uuid::new_v4();
    let socket_path = default_socket_path();

    // Notify locally and register with the daemon (best effort).
    notifier.notify(run_started(agent_kind, &working_dir, task.as_deref()))?;
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

    let stored = store_run(storage_config, run).await?;

    println!("Scotia captured run {}", stored.run_id);
    println!("  JSON:    {}", stored.json_path.display());
    println!("  Summary: {}", stored.summary_path.display());
    println!("  Graph:   {}", stored.dot_path.display());
    Ok(())
}

/// Resolve the agent program to run, honoring `--agent-path`, pinned paths in
/// `agents.json`, and `--no-path-fallback`. Returns an absolute path whenever
/// one is mandated so that a malicious earlier PATH entry cannot be executed.
fn resolve_agent_program(
    kind: AgentKind,
    command: &[String],
    agent_path: Option<PathBuf>,
    no_path_fallback: bool,
) -> Result<String> {
    if let Some(p) = agent_path {
        // Only reject paths that exist but are unsafe (e.g. group/other-writable).
        // A non-existent explicit path is harmless: letting it reach spawn yields
        // the OS "No such file" error, which is clearer than a safety rejection.
        if p.exists() && !crate::shim::is_safe_executable(&p) {
            anyhow::bail!(
                "--agent-path {} is not a safe executable (must not be group/other-writable)",
                p.display()
            );
        }
        return Ok(p.display().to_string());
    }

    let program = command.first().cloned().context("no command provided")?;
    let candidate = PathBuf::from(&program);
    if candidate.is_absolute() {
        if candidate.exists() && !crate::shim::is_safe_executable(&candidate) {
            anyhow::bail!(
                "agent program {} is not a safe executable (must not be group/other-writable)",
                program
            );
        }
        return Ok(program);
    }

    // Bare name: prefer a pinned absolute path from agents.json.
    if let Some(pinned) = crate::shim::pinned_agent_path(kind)
        && crate::shim::is_safe_executable(&pinned)
    {
        return Ok(pinned.display().to_string());
    }

    if no_path_fallback {
        anyhow::bail!(
            "--no-path-fallback is set and '{}' is not absolute and has no pinned path in {}",
            program,
            crate::shim::agent_pins_path().display()
        );
    }

    // Existing behaviour: let the OS resolve the bare name via PATH at spawn.
    Ok(program)
}
