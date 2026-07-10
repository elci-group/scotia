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

/// Returns true when `s` looks like a line belonging to a unified diff block.
/// Callers decide whether to test the raw line or a trimmed copy.
pub fn is_diff_line(s: &str) -> bool {
    s.starts_with("---")
        || s.starts_with("+++")
        || s.starts_with("@@")
        || s.starts_with('+')
        || s.starts_with('-')
        || s.starts_with(' ')
}

/// Classify an error/retry signal from a lowercased, trimmed line.
pub fn classify_error(lower: &str) -> Option<ErrorKind> {
    if lower.contains("retrying")
        || lower.contains("try again")
        || lower.starts_with("error:")
        || lower.starts_with("failed:")
    {
        Some(
            if lower.contains("retrying") || lower.contains("try again") {
                ErrorKind::Retry
            } else {
                ErrorKind::Unknown
            },
        )
    } else {
        None
    }
}

/// If a unified-diff block is in flight and the current line breaks it, emit the
/// accumulated state delta and clear the buffer. Returns the event to push, if any.
pub fn take_diff_if_broken(
    diff_buffer: &mut Option<(String, String)>,
    current_is_diff: bool,
    ctx: &InterceptorContext,
) -> Option<ScotiaEvent> {
    let (path, buf) = diff_buffer.as_mut()?;
    if !current_is_diff && !buf.is_empty() {
        let event = emit_state_delta(ctx, Some(path.clone()), Some(buf.clone()), None);
        *diff_buffer = None;
        Some(event)
    } else {
        None
    }
}

/// Append `line` to the in-flight diff buffer when it is a diff line. Returns
/// true if the line was consumed so callers can short-circuit.
pub fn accumulate_diff(
    diff_buffer: &mut Option<(String, String)>,
    line: &str,
    current_is_diff: bool,
) -> bool {
    let Some((_path, buf)) = diff_buffer.as_mut() else {
        return false;
    };
    if current_is_diff {
        buf.push_str(line);
        buf.push('\n');
        true
    } else {
        false
    }
}

/// Return a process-wide cached, compiled regex. Each call site declares its
/// own `static RE: OnceLock<Regex> = OnceLock::new();` and passes it here, so a
/// pattern is compiled at most once instead of on every parsed line.
pub fn cached_regex(
    slot: &'static std::sync::OnceLock<regex::Regex>,
    pattern: &'static str,
) -> &'static regex::Regex {
    slot.get_or_init(|| {
        regex::Regex::new(pattern).expect("interceptor regex pattern must be valid")
    })
}
