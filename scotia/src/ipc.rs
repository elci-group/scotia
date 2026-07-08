use crate::event::AgentKind;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Unix socket path for the Scotia daemon.
pub fn default_socket_path() -> PathBuf {
    dirs::runtime_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("scotiad.sock")
}

/// PID file path for the Scotia daemon.
pub fn default_pid_file() -> PathBuf {
    dirs::state_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("scotia")
        .join("scotiad.pid")
}

/// Log file path for the Scotia daemon.
pub fn default_log_file() -> PathBuf {
    dirs::state_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("scotia")
        .join("scotiad.log")
}

/// A request sent from a client (shim or CLI) to `scotiad`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum DaemonRequest {
    Ping,
    RegisterRun {
        run_id: Uuid,
        agent: AgentKind,
        task: Option<String>,
        cwd: PathBuf,
        started_at: DateTime<Utc>,
    },
    FinishRun {
        run_id: Uuid,
        exit_code: Option<i32>,
        actions: usize,
        models: usize,
        errors: usize,
        retries: usize,
        finished_at: DateTime<Utc>,
    },
    ListRuns,
}

/// A response from `scotiad` back to a client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DaemonResponse {
    Pong,
    Ok,
    Runs { runs: Vec<RunSummary> },
    Error { message: String },
}

/// A lightweight summary of an active or recently finished run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: Uuid,
    pub agent: AgentKind,
    pub task: Option<String>,
    pub cwd: PathBuf,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub actions: usize,
    pub models: usize,
    pub errors: usize,
    pub retries: usize,
}

impl RunSummary {
    pub fn is_active(&self) -> bool {
        self.finished_at.is_none()
    }

    pub fn duration(&self) -> chrono::Duration {
        let end = self.finished_at.unwrap_or_else(Utc::now);
        end.signed_duration_since(self.started_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::AgentKind;

    #[test]
    fn request_serializes_with_method_tag() {
        let req = DaemonRequest::Ping;
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"method\":\"ping\""));
    }

    #[test]
    fn run_summary_active_when_not_finished() {
        let summary = RunSummary {
            run_id: uuid::Uuid::new_v4(),
            agent: AgentKind::KimiCode,
            task: None,
            cwd: PathBuf::from("."),
            started_at: Utc::now(),
            finished_at: None,
            exit_code: None,
            actions: 0,
            models: 0,
            errors: 0,
            retries: 0,
        };
        assert!(summary.is_active());
    }

    #[test]
    fn run_summary_duration_non_negative() {
        let now = Utc::now();
        let summary = RunSummary {
            run_id: uuid::Uuid::new_v4(),
            agent: AgentKind::ClaudeCode,
            task: None,
            cwd: PathBuf::from("."),
            started_at: now,
            finished_at: Some(now + chrono::Duration::seconds(10)),
            exit_code: Some(0),
            actions: 1,
            models: 0,
            errors: 0,
            retries: 0,
        };
        let duration = summary.duration();
        assert!(duration.num_seconds() >= 10);
    }
}
