//! Canonical Scotia event types.
//!
//! Split into focused submodules; everything is re-exported here so the public
//! surface (`scotia::event::*`) is unchanged.

mod builders;
mod kind;

pub use builders::ScotiaRun;
pub use kind::{ActionStatus, AgentKind, ErrorKind, Role};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Unique identifier for a Scotia run.
pub type RunId = Uuid;

/// Unique identifier for an event within a run.
pub type EventId = Uuid;

/// A canonical Scotia event.
///
/// This is the minimal set of event types that can reconstruct an agent
/// execution without storing full transcripts. Each variant captures a
/// semantically meaningful state transition rather than raw bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScotiaEvent {
    /// Agent process started.
    RunStarted {
        run_id: RunId,
        agent: AgentKind,
        task: Option<String>,
        timestamp: DateTime<Utc>,
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        metadata: HashMap<String, serde_json::Value>,
    },

    /// User prompt or other high-level input submitted to the agent.
    PromptSubmitted {
        event_id: EventId,
        run_id: RunId,
        timestamp: DateTime<Utc>,
        role: Role,
        content: String,
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        context: HashMap<String, serde_json::Value>,
    },

    /// A tool or shell action was invoked by the agent.
    ActionInvoked {
        event_id: EventId,
        run_id: RunId,
        timestamp: DateTime<Utc>,
        tool: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        arguments: Option<serde_json::Value>,
    },

    /// Result returned from a tool or shell action.
    ActionResult {
        event_id: EventId,
        run_id: RunId,
        timestamp: DateTime<Utc>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<ActionStatus>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stdout: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stderr: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
    },

    /// Model routing decision, e.g. planner -> Groq, executor -> Ollama.
    ModelRouted {
        event_id: EventId,
        run_id: RunId,
        timestamp: DateTime<Utc>,
        stage: String,
        model: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        latency_ms: Option<u64>,
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        metadata: HashMap<String, serde_json::Value>,
    },

    /// Agent emitted a structured or free-form response chunk.
    ResponseChunk {
        event_id: EventId,
        run_id: RunId,
        timestamp: DateTime<Utc>,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        finish_reason: Option<String>,
    },

    /// Error, retry, or correction event.
    ErrorOrRetry {
        event_id: EventId,
        run_id: RunId,
        timestamp: DateTime<Utc>,
        kind: ErrorKind,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_count: Option<u32>,
    },

    /// Environment state change detected (file write, git operation, etc.).
    StateDelta {
        event_id: EventId,
        run_id: RunId,
        timestamp: DateTime<Utc>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diff: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },

    /// Agent process ended.
    RunFinished {
        run_id: RunId,
        timestamp: DateTime<Utc>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
}

impl ScotiaEvent {
    pub fn run_id(&self) -> RunId {
        match self {
            ScotiaEvent::RunStarted { run_id, .. } => *run_id,
            ScotiaEvent::PromptSubmitted { run_id, .. } => *run_id,
            ScotiaEvent::ActionInvoked { run_id, .. } => *run_id,
            ScotiaEvent::ActionResult { run_id, .. } => *run_id,
            ScotiaEvent::ModelRouted { run_id, .. } => *run_id,
            ScotiaEvent::ResponseChunk { run_id, .. } => *run_id,
            ScotiaEvent::ErrorOrRetry { run_id, .. } => *run_id,
            ScotiaEvent::StateDelta { run_id, .. } => *run_id,
            ScotiaEvent::RunFinished { run_id, .. } => *run_id,
        }
    }

    pub fn timestamp(&self) -> DateTime<Utc> {
        match self {
            ScotiaEvent::RunStarted { timestamp, .. } => *timestamp,
            ScotiaEvent::PromptSubmitted { timestamp, .. } => *timestamp,
            ScotiaEvent::ActionInvoked { timestamp, .. } => *timestamp,
            ScotiaEvent::ActionResult { timestamp, .. } => *timestamp,
            ScotiaEvent::ModelRouted { timestamp, .. } => *timestamp,
            ScotiaEvent::ResponseChunk { timestamp, .. } => *timestamp,
            ScotiaEvent::ErrorOrRetry { timestamp, .. } => *timestamp,
            ScotiaEvent::StateDelta { timestamp, .. } => *timestamp,
            ScotiaEvent::RunFinished { timestamp, .. } => *timestamp,
        }
    }
}
