use crate::event::AgentKind;
use crate::ipc::RunSummary;
use crate::notify::{Notifier, run_crashed, run_finished, run_started, run_still_active};
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

mod server;
pub use server::handle_client;

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
        runs.sort_by_key(|b| std::cmp::Reverse(b.started_at));
        runs
    }

    pub fn active_runs(&self) -> Vec<&RunSummary> {
        self.runs.values().filter(|r| r.is_active()).collect()
    }

    pub fn prune_finished(&mut self, older_than: chrono::Duration) {
        let cutoff = Utc::now() - older_than;
        self.runs
            .retain(|_, run| run.finished_at.map(|t| t > cutoff).unwrap_or(true));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::AgentKind;
    use crate::notify::TestNotifier;
    use std::sync::Arc;
    use std::time::Duration;
    use uuid::Uuid;

    fn test_daemon() -> Daemon {
        let notifier: Arc<dyn Notifier> = Arc::new(TestNotifier::new());
        Daemon::new(notifier, 3600)
    }

    #[tokio::test]
    async fn register_and_finish_run() {
        let daemon = test_daemon();
        let run_id = Uuid::new_v4();
        let cwd = std::path::PathBuf::from("/tmp/project");

        daemon
            .register_run(
                run_id,
                AgentKind::KimiCode,
                Some("refactor".to_string()),
                cwd.clone(),
            )
            .await
            .unwrap();

        let state = daemon.state.read().await;
        let active = state.active_runs();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].agent, AgentKind::KimiCode);
        drop(state);

        daemon
            .finish_run(run_id, Some(0), 5, 2, 0, 0)
            .await
            .unwrap();

        let runs = daemon.list_runs().await;
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].actions, 5);
        assert_eq!(runs[0].models, 2);
        assert!(runs[0].finished_at.is_some());
    }

    #[tokio::test]
    async fn finish_unknown_run_is_noop() {
        let daemon = test_daemon();
        let result = daemon.finish_run(Uuid::new_v4(), Some(0), 0, 0, 0, 0).await;
        assert!(result.is_ok());
        assert!(daemon.list_runs().await.is_empty());
    }

    #[tokio::test]
    async fn prune_removes_old_finished_runs() {
        let daemon = test_daemon();
        let run_id = Uuid::new_v4();
        daemon
            .register_run(
                run_id,
                AgentKind::ClaudeCode,
                None,
                std::path::PathBuf::from("."),
            )
            .await
            .unwrap();
        daemon
            .finish_run(run_id, Some(0), 1, 0, 0, 0)
            .await
            .unwrap();

        // Fake the finished_at time to be far in the past.
        {
            let mut state = daemon.state.write().await;
            if let Some(run) = state.runs.get_mut(&run_id) {
                run.finished_at = Some(Utc::now() - chrono::Duration::hours(2));
            }
        }

        daemon.prune(chrono::Duration::minutes(30)).await;
        assert!(daemon.list_runs().await.is_empty());
    }

    #[tokio::test]
    async fn progress_notification_for_long_run() {
        let notifier = Arc::new(TestNotifier::new());
        let daemon = Daemon::new(notifier.clone(), 1);
        let run_id = Uuid::new_v4();
        daemon
            .register_run(
                run_id,
                AgentKind::Codex,
                None,
                std::path::PathBuf::from("."),
            )
            .await
            .unwrap();

        // Manually backdate the start time so the run appears long-running.
        {
            let mut state = daemon.state.write().await;
            if let Some(run) = state.runs.get_mut(&run_id) {
                run.started_at = Utc::now() - chrono::Duration::seconds(5);
            }
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
        daemon.tick_progress_notifications().await.unwrap();

        assert!(
            notifier.contains(|n| matches!(n.level, crate::notify::NotificationLevel::IceField))
        );
    }
}
