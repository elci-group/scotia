use crate::event::{ErrorKind, ScotiaEvent};
use crate::interceptor::{AgentInterceptor, InterceptorContext, StreamSource};
use crate::interceptors::{
    classify_stderr, emit_action_invoked, emit_error_or_retry, emit_model_routed,
    emit_response_chunk, emit_state_delta,
};
use regex::Regex;
use std::sync::OnceLock;

/// Interceptor for Claude Code telemetry.
///
/// Parses tool invocations (e.g. `› Read: src/main.rs`), model routing hints,
/// unified-diff edit blocks, retry/error signals, and free-form response chunks.
#[derive(Default)]
pub struct ClaudeInterceptor {
    /// Accumulated diff block after an `edit` or `write` action: (path, diff).
    diff_buffer: Option<(String, String)>,
}

impl AgentInterceptor for ClaudeInterceptor {
    fn name(&self) -> &'static str {
        "claude"
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

        // If we are accumulating a diff block, consume diff-looking lines or close it.
        if let Some((path, buf)) = self.diff_buffer.as_mut() {
            if is_diff_line(trimmed) {
                buf.push_str(line);
                buf.push('\n');
                return events;
            }

            if !trimmed.is_empty() {
                if buf.len() > 40 {
                    events.push(emit_state_delta(
                        ctx,
                        Some(path.clone()),
                        Some(buf.clone()),
                        None,
                    ));
                }
                self.diff_buffer = None;
                // fall through to classify the current line
            } else {
                // Blank line inside a diff block: keep accumulating.
                return events;
            }
        }

        // Tool/action invocations: "› Read: src/main.rs", "> Bash: cargo test", etc.
        if let Some(cap) = tool_regex().captures(trimmed) {
            let tool = cap[1].to_lowercase();
            let rest = cap[2].to_string();
            events.push(emit_action_invoked(
                ctx,
                tool.clone(),
                Some(rest.clone()),
                None,
            ));

            if tool == "edit" || tool == "write" {
                self.diff_buffer = Some((rest, String::new()));
            }
            return events;
        }

        // Alternative tool-use phrasing: "Using tool: read_file".
        if let Some(cap) = alt_tool_regex().captures(trimmed) {
            let tool = cap[1].to_lowercase();
            let rest = cap.get(2).map(|m| m.as_str().to_string());
            events.push(emit_action_invoked(ctx, tool, rest, None));
            return events;
        }

        // Model routing decisions.
        if let Some(cap) = model_regex().captures(trimmed) {
            let model = cap[1].to_string();
            if model.to_lowercase() != "claude" {
                events.push(emit_model_routed(ctx, "inference", model, None));
                return events;
            }
        }

        // Retry / error signals on stdout.
        let lower = trimmed.to_lowercase();
        if lower.contains("retrying") || lower.contains("try again") {
            events.push(emit_error_or_retry(
                ctx,
                ErrorKind::Retry,
                trimmed.to_string(),
                None,
            ));
            return events;
        }
        if lower.starts_with("error:") || lower.contains("error occurred") {
            events.push(emit_error_or_retry(
                ctx,
                ErrorKind::Unknown,
                trimmed.to_string(),
                None,
            ));
            return events;
        }

        // Everything else that is non-empty is a response chunk.
        if !trimmed.is_empty() {
            events.push(emit_response_chunk(ctx, line.to_string()));
        }

        events
    }

    fn finalize(&mut self, ctx: &InterceptorContext, exit_code: Option<i32>) -> Vec<ScotiaEvent> {
        let mut events = Vec::new();
        if let Some((path, buf)) = self.diff_buffer.take() {
            if buf.len() > 40 {
                events.push(emit_state_delta(ctx, Some(path), Some(buf), None));
            }
        }
        events.push(ScotiaEvent::RunFinished {
            run_id: ctx.run_id,
            timestamp: chrono::Utc::now(),
            exit_code,
            summary: None,
        });
        events
    }
}

fn is_diff_line(line: &str) -> bool {
    line.starts_with("---")
        || line.starts_with("+++")
        || line.starts_with("@@")
        || line.starts_with('+')
        || line.starts_with('-')
        || line.starts_with(' ')
}

fn tool_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[›>•]\s*(\w+):\s*(.+)$").unwrap())
}

fn alt_tool_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^using\s+tool\s*[:=]?\s*(\w+)(?:\s*[:=]\s*(.+))?$").unwrap())
}

fn model_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)(?:using\s+model|routed\s+to|routing\s+to|model\s*:\s*)\s*([a-z0-9_-]+)")
            .unwrap()
    })
}
