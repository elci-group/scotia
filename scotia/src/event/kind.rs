//! Small "kind" enums shared by every event: agent, role, action status, error.

use serde::{Deserialize, Serialize};

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
