use crate::event::{AgentKind, EventId, ScotiaEvent};
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Context shared by all adapters while parsing a live stream.
#[derive(Debug, Clone)]
pub struct AdapterContext {
    pub run_id: Uuid,
    pub agent: AgentKind,
}

/// A streaming parser that turns raw agent output into canonical Scotia events.
///
/// Adapters are stateful: they may accumulate lines across chunks to detect
/// multi-line tool invocations, diff blocks, or routing annotations.
pub trait AgentAdapter: Send + Sync {
    /// Parse a single line (or chunk) of raw output and emit zero or more events.
    ///
    /// `source` indicates whether the line came from the agent's stdout or stderr.
    fn parse_line(
        &mut self,
        ctx: &AdapterContext,
        source: StreamSource,
        line: &str,
    ) -> Vec<ScotiaEvent>;

    /// Called when the wrapped process exits.
    fn finalize(&mut self, ctx: &AdapterContext, exit_code: Option<i32>) -> Vec<ScotiaEvent> {
        vec![ScotiaEvent::RunFinished {
            run_id: ctx.run_id,
            timestamp: Utc::now(),
            exit_code,
            summary: None,
        }]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamSource {
    Stdout,
    Stderr,
}

pub type SharedAdapter = Arc<Mutex<dyn AgentAdapter>>;

impl<T: AgentAdapter + ?Sized> AgentAdapter for Box<T> {
    fn parse_line(
        &mut self,
        ctx: &AdapterContext,
        source: StreamSource,
        line: &str,
    ) -> Vec<ScotiaEvent> {
        (**self).parse_line(ctx, source, line)
    }

    fn finalize(&mut self, ctx: &AdapterContext, exit_code: Option<i32>) -> Vec<ScotiaEvent> {
        (**self).finalize(ctx, exit_code)
    }
}

/// Build an adapter for the given agent kind.
pub fn build_adapter(agent: AgentKind) -> Box<dyn AgentAdapter> {
    match agent {
        AgentKind::ClaudeCode => Box::new(crate::adapters::claude::ClaudeAdapter::default()),
        AgentKind::Codex => Box::new(crate::adapters::codex::CodexAdapter::default()),
        AgentKind::KimiCode => Box::new(crate::adapters::kimi::KimiAdapter::default()),
        AgentKind::Agy => Box::new(crate::adapters::agy::AgyAdapter::default()),
        AgentKind::Cosine => Box::new(crate::adapters::cosine::CosineAdapter::default()),
        AgentKind::Opencode => Box::new(crate::adapters::opencode::OpencodeAdapter::default()),
        AgentKind::Unknown => Box::new(crate::adapters::generic::GenericAdapter::default()),
    }
}

/// Convenience: create an event id for the current run.
pub fn new_event_id() -> EventId {
    Uuid::new_v4()
}
