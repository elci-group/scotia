use crate::event::{AgentKind, ScotiaEvent};
use std::collections::HashMap;

/// Shared context supplied to every interceptor.
#[derive(Debug, Clone)]
pub struct InterceptorContext {
    pub run_id: uuid::Uuid,
    pub agent: AgentKind,
    /// Extra hints, e.g. known working directory or expected log paths.
    pub hints: HashMap<String, String>,
}

/// Source of a captured byte stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamSource {
    Stdout,
    Stderr,
    /// Agent-specific side channel, e.g. a structured log file or MCP message.
    SideChannel,
}

/// A stateful interceptor that turns raw agent telemetry into canonical Scotia events.
///
/// Interceptors are transport-aware parsers: they know how an agent surfaces
/// tool calls, routing decisions, edits, and errors, and they reconstruct those
/// as a structured event graph.
pub trait AgentInterceptor: Send + Sync {
    /// Human-readable name of the interceptor.
    fn name(&self) -> &'static str;

    /// Parse one line/frame from the given source and emit zero or more events.
    fn parse_line(
        &mut self,
        ctx: &InterceptorContext,
        source: StreamSource,
        line: &str,
    ) -> Vec<ScotiaEvent>;

    /// Optional: consume a side-channel payload (structured JSON, MCP message, etc.).
    fn parse_side_channel(&mut self, ctx: &InterceptorContext, payload: &str) -> Vec<ScotiaEvent> {
        self.parse_line(ctx, StreamSource::SideChannel, payload)
    }

    /// Called once the wrapped process or side channel has closed.
    fn finalize(&mut self, ctx: &InterceptorContext, exit_code: Option<i32>) -> Vec<ScotiaEvent> {
        vec![ScotiaEvent::RunFinished {
            run_id: ctx.run_id,
            timestamp: chrono::Utc::now(),
            exit_code,
            summary: None,
        }]
    }
}

pub type BoxedInterceptor = Box<dyn AgentInterceptor>;

/// Build the interceptor for a given agent kind.
pub fn build_interceptor(agent: AgentKind) -> BoxedInterceptor {
    match agent {
        AgentKind::KimiCode => Box::new(crate::interceptors::kimi::KimiInterceptor::default()),
        AgentKind::Agy => Box::new(crate::interceptors::agy::AgyInterceptor::default()),
        AgentKind::Cosine => Box::new(crate::interceptors::cosine::CosineInterceptor),
        AgentKind::Codex => Box::new(crate::interceptors::codex::CodexInterceptor::default()),
        AgentKind::ClaudeCode => {
            Box::new(crate::interceptors::claude::ClaudeInterceptor::default())
        }
        AgentKind::Opencode => {
            Box::new(crate::interceptors::opencode::OpencodeInterceptor::default())
        }
        AgentKind::Unknown => Box::new(crate::interceptors::GenericInterceptor),
    }
}

impl<T: AgentInterceptor + ?Sized> AgentInterceptor for Box<T> {
    fn name(&self) -> &'static str {
        (**self).name()
    }

    fn parse_line(
        &mut self,
        ctx: &InterceptorContext,
        source: StreamSource,
        line: &str,
    ) -> Vec<ScotiaEvent> {
        (**self).parse_line(ctx, source, line)
    }

    fn parse_side_channel(&mut self, ctx: &InterceptorContext, payload: &str) -> Vec<ScotiaEvent> {
        (**self).parse_side_channel(ctx, payload)
    }

    fn finalize(&mut self, ctx: &InterceptorContext, exit_code: Option<i32>) -> Vec<ScotiaEvent> {
        (**self).finalize(ctx, exit_code)
    }
}

/// Convenience helper to mint fresh event ids.
pub fn new_event_id() -> uuid::Uuid {
    uuid::Uuid::new_v4()
}
