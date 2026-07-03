pub mod agy;
pub mod claude;
pub mod codex;
pub mod cosine;
pub mod generic;
pub mod kimi;
pub mod opencode;

use crate::adapter::AdapterContext;
/// Shared parsing helpers for agent adapters.
use crate::event::{ActionStatus, ErrorKind, ScotiaEvent};
use chrono::Utc;

pub fn emit_action_invoked(
    ctx: &AdapterContext,
    tool: impl Into<String>,
    target: Option<String>,
    arguments: Option<serde_json::Value>,
) -> ScotiaEvent {
    ScotiaEvent::ActionInvoked {
        event_id: crate::adapter::new_event_id(),
        run_id: ctx.run_id,
        timestamp: Utc::now(),
        tool: tool.into(),
        target,
        arguments,
    }
}

pub fn emit_action_result(
    ctx: &AdapterContext,
    status: Option<ActionStatus>,
    stdout: Option<String>,
    stderr: Option<String>,
    exit_code: Option<i32>,
) -> ScotiaEvent {
    ScotiaEvent::ActionResult {
        event_id: crate::adapter::new_event_id(),
        run_id: ctx.run_id,
        timestamp: Utc::now(),
        status,
        stdout,
        stderr,
        exit_code,
    }
}

pub fn emit_response_chunk(ctx: &AdapterContext, content: impl Into<String>) -> ScotiaEvent {
    ScotiaEvent::ResponseChunk {
        event_id: crate::adapter::new_event_id(),
        run_id: ctx.run_id,
        timestamp: Utc::now(),
        content: content.into(),
        finish_reason: None,
    }
}

pub fn emit_error_or_retry(
    ctx: &AdapterContext,
    kind: ErrorKind,
    message: impl Into<String>,
    retry_count: Option<u32>,
) -> ScotiaEvent {
    ScotiaEvent::ErrorOrRetry {
        event_id: crate::adapter::new_event_id(),
        run_id: ctx.run_id,
        timestamp: Utc::now(),
        kind,
        message: message.into(),
        retry_count,
    }
}

pub fn emit_state_delta(
    ctx: &AdapterContext,
    path: Option<String>,
    diff: Option<String>,
    description: Option<String>,
) -> ScotiaEvent {
    ScotiaEvent::StateDelta {
        event_id: crate::adapter::new_event_id(),
        run_id: ctx.run_id,
        timestamp: Utc::now(),
        path,
        diff,
        description,
    }
}

/// Heuristic: treat stderr lines as errors unless they look like progress output.
pub fn classify_stderr(_ctx: &AdapterContext, line: &str) -> Option<ScotiaEvent> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Many agents print progress/status to stderr; ignore very short lines.
    if trimmed.len() < 8 {
        return None;
    }
    Some(emit_error_or_retry(
        _ctx,
        ErrorKind::Unknown,
        trimmed.to_string(),
        None,
    ))
}
