//! `ScotiaRun` and its constructors (`new` / `push` / `finish`).

use std::collections::HashMap;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{AgentKind, RunId, ScotiaEvent};

/// A complete Scotia run, stored as a single artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScotiaRun {
    pub run_id: RunId,
    pub agent: AgentKind,
    pub task: Option<String>,
    pub started_at: chrono::DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<chrono::DateTime<Utc>>,
    pub events: Vec<ScotiaEvent>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl ScotiaRun {
    pub fn new(agent: AgentKind, task: Option<String>, run_id: Option<Uuid>) -> Self {
        let run_id = run_id.unwrap_or_else(Uuid::new_v4);
        let started_at = Utc::now();
        Self {
            run_id,
            agent,
            task: task.clone(),
            started_at,
            finished_at: None,
            events: vec![ScotiaEvent::RunStarted {
                run_id,
                agent,
                task,
                timestamp: started_at,
                metadata: HashMap::new(),
            }],
            metadata: HashMap::new(),
        }
    }

    pub fn push(&mut self, event: ScotiaEvent) {
        self.events.push(event);
    }

    pub fn finish(&mut self, exit_code: Option<i32>, summary: Option<String>) {
        let timestamp = Utc::now();
        self.finished_at = Some(timestamp);
        self.push(ScotiaEvent::RunFinished {
            run_id: self.run_id,
            timestamp,
            exit_code,
            summary,
        });
    }
}
