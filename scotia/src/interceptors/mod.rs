pub mod agy;
pub mod claude;
pub mod codex;
pub mod cosine;
pub mod kimi;
pub mod opencode;

use crate::event::{ActionStatus, ErrorKind, Role, ScotiaEvent};
use crate::interceptor::{AgentInterceptor, InterceptorContext, StreamSource};
use chrono::Utc;

/// Fallback interceptor for agents that do not yet have a dedicated parser.
#[derive(Default)]
pub struct GenericInterceptor;

impl AgentInterceptor for GenericInterceptor {
    fn name(&self) -> &'static str {
        "generic"
    }

    fn parse_line(
        &mut self,
        ctx: &InterceptorContext,
        source: StreamSource,
        line: &str,
    ) -> Vec<ScotiaEvent> {
        if source == StreamSource::Stderr {
            return classify_stderr(ctx, line).into_iter().collect();
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            Vec::new()
        } else {
            vec![emit_response_chunk(ctx, line.to_string())]
        }
    }
}

pub fn emit_run_started(ctx: &InterceptorContext, task: Option<String>) -> ScotiaEvent {
    ScotiaEvent::RunStarted {
        run_id: ctx.run_id,
        agent: ctx.agent,
        task,
        timestamp: Utc::now(),
        metadata: Default::default(),
    }
}

pub fn emit_prompt(
    ctx: &InterceptorContext,
    role: Role,
    content: impl Into<String>,
) -> ScotiaEvent {
    ScotiaEvent::PromptSubmitted {
        event_id: crate::interceptor::new_event_id(),
        run_id: ctx.run_id,
        timestamp: Utc::now(),
        role,
        content: content.into(),
        context: Default::default(),
    }
}

pub fn emit_action_invoked(
    ctx: &InterceptorContext,
    tool: impl Into<String>,
    target: Option<String>,
    arguments: Option<serde_json::Value>,
) -> ScotiaEvent {
    ScotiaEvent::ActionInvoked {
        event_id: crate::interceptor::new_event_id(),
        run_id: ctx.run_id,
        timestamp: Utc::now(),
        tool: tool.into(),
        target,
        arguments,
    }
}

pub fn emit_action_result(
    ctx: &InterceptorContext,
    status: Option<ActionStatus>,
    stdout: Option<String>,
    stderr: Option<String>,
    exit_code: Option<i32>,
) -> ScotiaEvent {
    ScotiaEvent::ActionResult {
        event_id: crate::interceptor::new_event_id(),
        run_id: ctx.run_id,
        timestamp: Utc::now(),
        status,
        stdout,
        stderr,
        exit_code,
    }
}

pub fn emit_model_routed(
    ctx: &InterceptorContext,
    stage: impl Into<String>,
    model: impl Into<String>,
    latency_ms: Option<u64>,
) -> ScotiaEvent {
    ScotiaEvent::ModelRouted {
        event_id: crate::interceptor::new_event_id(),
        run_id: ctx.run_id,
        timestamp: Utc::now(),
        stage: stage.into(),
        model: model.into(),
        latency_ms,
        metadata: Default::default(),
    }
}

pub fn emit_response_chunk(ctx: &InterceptorContext, content: impl Into<String>) -> ScotiaEvent {
    ScotiaEvent::ResponseChunk {
        event_id: crate::interceptor::new_event_id(),
        run_id: ctx.run_id,
        timestamp: Utc::now(),
        content: content.into(),
        finish_reason: None,
    }
}

pub fn emit_error_or_retry(
    ctx: &InterceptorContext,
    kind: ErrorKind,
    message: impl Into<String>,
    retry_count: Option<u32>,
) -> ScotiaEvent {
    ScotiaEvent::ErrorOrRetry {
        event_id: crate::interceptor::new_event_id(),
        run_id: ctx.run_id,
        timestamp: Utc::now(),
        kind,
        message: message.into(),
        retry_count,
    }
}

pub fn emit_state_delta(
    ctx: &InterceptorContext,
    path: Option<String>,
    diff: Option<String>,
    description: Option<String>,
) -> ScotiaEvent {
    ScotiaEvent::StateDelta {
        event_id: crate::interceptor::new_event_id(),
        run_id: ctx.run_id,
        timestamp: Utc::now(),
        path,
        diff,
        description,
    }
}

/// Heuristic stderr classifier: treats longer stderr lines as potential errors,
/// but suppresses short progress noise.
pub fn classify_stderr(ctx: &InterceptorContext, line: &str) -> Option<ScotiaEvent> {
    let trimmed = line.trim();
    if trimmed.len() < 8 {
        return None;
    }
    Some(emit_error_or_retry(ctx, ErrorKind::Unknown, trimmed, None))
}
