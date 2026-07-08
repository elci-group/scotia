use super::*;
use crate::event::{ErrorKind, ScotiaEvent};
use crate::interceptor::{AgentInterceptor, InterceptorContext, StreamSource};
use regex::Regex;

/// Interceptor for the `agy` agent.
///
/// agy emits structured JSON tool calls, plain-text action annotations,
/// model routing hints, and occasional unified-diff blocks. This interceptor
/// turns each of those into canonical Scotia events.
#[derive(Default)]
pub struct AgyInterceptor {
    /// Accumulated multi-line diff block when an edit/write is in flight.
    /// Stores `(path, accumulated_diff)`.
    diff_buffer: Option<(String, String)>,
}

impl AgentInterceptor for AgyInterceptor {
    fn name(&self) -> &'static str {
        "agy"
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

        let mut events = Vec::new();
        let trimmed = line.trim();

        // Flush any in-progress diff block if the current line breaks it.
        if let Some((path, buf)) = self.diff_buffer.as_mut() {
            let is_diff_line = line.starts_with("---")
                || line.starts_with("+++")
                || line.starts_with("@@")
                || line.starts_with('+')
                || line.starts_with('-')
                || line.starts_with(' ');

            if !is_diff_line && !buf.is_empty() {
                events.push(emit_state_delta(
                    ctx,
                    Some(path.clone()),
                    Some(buf.clone()),
                    None,
                ));
                self.diff_buffer = None;
            }
        }

        // Structured JSON tool invocation:
        // {"tool":"file_read","target":"src/main.rs","arguments":{...}}
        if trimmed.starts_with('{')
            && trimmed.ends_with('}')
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed)
            && let Some(tool) = value.get("tool").and_then(|v| v.as_str())
        {
            let target = value
                .get("target")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let arguments = value.get("arguments").cloned();

            if tool.eq_ignore_ascii_case("edit")
                || tool.eq_ignore_ascii_case("write")
                || tool.eq_ignore_ascii_case("apply_patch")
            {
                let path = value
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| target.clone())
                    .unwrap_or_default();
                self.diff_buffer = Some((path, String::new()));
            }

            events.push(emit_action_invoked(ctx, tool, target, arguments));
            return events;
        }

        // Plain-text action annotation: "tool: bash cargo test" or "action: grep".
        if let Some(cap) = Regex::new(r"(?i)(?:tool|action):\s*(\w+)(?:\s+(.+))?$")
            .unwrap()
            .captures(trimmed)
        {
            let tool = cap[1].to_string();
            let target = cap.get(2).map(|m| m.as_str().to_string());

            if tool.eq_ignore_ascii_case("edit") || tool.eq_ignore_ascii_case("write") {
                self.diff_buffer = Some((target.clone().unwrap_or_default(), String::new()));
            }

            events.push(emit_action_invoked(ctx, tool, target, None));
            return events;
        }

        // Model routing decision: groq, ollama, openai, local, etc.
        if let Some(cap) = Regex::new(
            r"(?i)(?:routing to\s+|routed to\s+|using model[:\s]+|model[:\s]+)([a-z0-9_.-]+)$",
        )
        .unwrap()
        .captures(trimmed)
        {
            let model = cap[1].to_string();
            events.push(emit_model_routed(ctx, "inference", model, None));
            return events;
        }

        // Error / retry signals.
        let lower = trimmed.to_lowercase();
        if lower.contains("retrying")
            || lower.contains("try again")
            || lower.starts_with("error:")
            || lower.starts_with("failed:")
        {
            let kind = if lower.contains("retrying") || lower.contains("try again") {
                ErrorKind::Retry
            } else {
                ErrorKind::Unknown
            };
            events.push(emit_error_or_retry(ctx, kind, trimmed.to_string(), None));
            return events;
        }

        // Accumulate unified-diff blocks after an edit/write.
        if let Some((_path, buf)) = self.diff_buffer.as_mut()
            && (line.starts_with("---")
                || line.starts_with("+++")
                || line.starts_with("@@")
                || line.starts_with('+')
                || line.starts_with('-')
                || line.starts_with(' '))
        {
            buf.push_str(line);
            buf.push('\n');
            return events;
        }

        // Anything else is a response chunk.
        if !trimmed.is_empty() {
            events.push(emit_response_chunk(ctx, line.to_string()));
        }

        events
    }
}
