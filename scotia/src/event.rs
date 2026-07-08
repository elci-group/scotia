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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    KimiCode,
    Agy,
    Cosine,
    Codex,
    ClaudeCode,
    Opencode,
    #[serde(other)]
    Unknown,
}

impl AgentKind {
    pub fn from_binary_name(name: &str) -> Self {
        let lower = name.to_lowercase();
        match lower.as_str() {
            "kimi" | "kimi-code" | "kimi_code" => AgentKind::KimiCode,
            "agy" => AgentKind::Agy,
            "cosine" => AgentKind::Cosine,
            "codex" | "codex-cli" | "codex_cli" => AgentKind::Codex,
            "claude" | "claude-code" | "claude_code" => AgentKind::ClaudeCode,
            "opencode" => AgentKind::Opencode,
            _ => AgentKind::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            AgentKind::KimiCode => "kimi-code",
            AgentKind::Agy => "agy",
            AgentKind::Cosine => "cosine",
            AgentKind::Codex => "codex",
            AgentKind::ClaudeCode => "claude-code",
            AgentKind::Opencode => "opencode",
            AgentKind::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    System,
    Agent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Success,
    Failure,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    ToolError,
    ModelError,
    Timeout,
    Retry,
    Unknown,
}

/// A complete Scotia run, stored as a single artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScotiaRun {
    pub run_id: RunId,
    pub agent: AgentKind,
    pub task: Option<String>,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
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
