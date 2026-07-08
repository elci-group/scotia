use crate::event::AgentKind;
use crate::ipc::RunSummary;
use crate::notify::{Notifier, run_crashed, run_finished, run_started, run_still_active};
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// In-memory state kept by `scotiad`.
#[derive(Debug, Default)]
pub struct DaemonState {
    runs: HashMap<Uuid, RunSummary>,
}

impl DaemonState {
    pub fn register_run(
        &mut self,
        run_id: Uuid,
        agent: AgentKind,
        task: Option<String>,
        cwd: std::path::PathBuf,
        started_at: DateTime<Utc>,
    ) {
        self.runs.insert(
            run_id,
            RunSummary {
                run_id,
                agent,
                task,
                cwd,
                started_at,
                finished_at: None,
                exit_code: None,
                actions: 0,
                models: 0,
                errors: 0,
                retries: 0,
            },
        );
    }

    pub fn finish_run(
        &mut self,
        run_id: Uuid,
        exit_code: Option<i32>,
        actions: usize,
        models: usize,
        errors: usize,
        retries: usize,
    ) -> Option<RunSummary> {
        if let Some(run) = self.runs.get_mut(&run_id) {
            run.finished_at = Some(Utc::now());
            run.exit_code = exit_code;
            run.actions = actions;
            run.models = models;
            run.errors = errors;
            run.retries = retries;
            return Some(run.clone());
        }
        None
    }

    pub fn list_runs(&self) -> Vec<RunSummary> {
        let mut runs: Vec<_> = self.runs.values().cloned().collect();
        runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        runs
    }

    pub fn active_runs(&self) -> Vec<&RunSummary> {
        self.runs.values().filter(|r| r.is_active()).collect()
    }

    pub fn prune_finished(&mut self, older_than: chrono::Duration) {
        let cutoff = Utc::now() - older_than;
        self.runs.retain(|_, run| {
            run.finished_at.map(|t| t > cutoff).unwrap_or(true)
        });
    }
}

/// Shared daemon state plus notification sink.
#[derive(Clone)]
pub struct Daemon {
    state: Arc<RwLock<DaemonState>>,
    notifier: Arc<dyn Notifier>,
    progress_interval_seconds: u64,
}

impl Daemon {
    pub fn new(notifier: Arc<dyn Notifier>, progress_interval_seconds: u64) -> Self {
        Self {
            state: Arc::new(RwLock::new(DaemonState::default())),
            notifier,
            progress_interval_seconds,
        }
    }

    pub async fn register_run(
        &self,
        run_id: Uuid,
        agent: AgentKind,
        task: Option<String>,
        cwd: std::path::PathBuf,
    ) -> Result<()> {
        let started_at = Utc::now();
        {
            let mut state = self.state.write().await;
            state.register_run(run_id, agent, task.clone(), cwd.clone(), started_at);
        }
        self.notifier
            .notify(run_started(agent, &cwd, task.as_deref()))?;
        Ok(())
    }

    pub async fn finish_run(
        &self,
        run_id: Uuid,
        exit_code: Option<i32>,
        actions: usize,
        models: usize,
        errors: usize,
        retries: usize,
    ) -> Result<()> {
        let run = {
            let mut state = self.state.write().await;
            state.finish_run(run_id, exit_code, actions, models, errors, retries)
        };
        if let Some(run) = run {
            let note = if exit_code.map(|c| c != 0).unwrap_or(true) && errors > 0 {
                run_crashed(run.agent, exit_code)
            } else {
                run_finished(run.agent, actions, models, errors, retries)
            };
            self.notifier.notify(note)?;
        }
        Ok(())
    }

    pub async fn list_runs(&self) -> Vec<RunSummary> {
        let state = self.state.read().await;
        state.list_runs()
    }

    /// Emit progress notifications for runs that have been active too long.
    pub async fn tick_progress_notifications(&self) -> Result<()> {
        let active = {
            let state = self.state.read().await;
            state.active_runs().into_iter().cloned().collect::<Vec<_>>()
        };
        let threshold = chrono::Duration::seconds(self.progress_interval_seconds as i64);
        for run in active {
            if run.duration() > threshold {
                self.notifier.notify(run_still_active(
                    run.agent,
                    run.duration().to_std().unwrap_or_default(),
                ))?;
            }
        }
        Ok(())
    }

    pub async fn prune(&self, older_than: chrono::Duration) {
        let mut state = self.state.write().await;
        state.prune_finished(older_than);
    }
}
