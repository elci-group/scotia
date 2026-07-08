use crate::event::{ErrorKind, ScotiaEvent};
use crate::interceptor::{AgentInterceptor, InterceptorContext, StreamSource};
use crate::interceptors::{
    classify_stderr, emit_action_invoked, emit_error_or_retry, emit_model_routed,
    emit_response_chunk, emit_state_delta,
};
use regex::Regex;
use std::sync::OnceLock;

/// Tools Kimi Code is known to emit in its transcript.
const KNOWN_TOOLS: &[&str] = &[
    "read",
    "file_read",
    "edit",
    "file_edit",
    "write",
    "bash",
    "shell",
    "grep",
    "search",
    "replace",
    "apply",
    "execute",
    "run",
    "cmd",
    "command",
    "python",
    "node",
    "cargo",
    "npm",
    "git",
    "curl",
    "cat",
    "ls",
    "mkdir",
    "mv",
    "cp",
    "rm",
    "touch",
];

/// Stateful interceptor for Kimi Code agent telemetry.
#[derive(Default)]
pub struct KimiInterceptor {
    /// Accumulated multi-line diff block: `(path, content)`.
    diff_buffer: Option<(String, String)>,
}

impl KimiInterceptor {
    pub fn new() -> Self {
        Self::default()
    }
}

impl AgentInterceptor for KimiInterceptor {
    fn name(&self) -> &'static str {
        "kimi"
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

        // If we are in the middle of accumulating a diff block, continue or flush it.
        if let Some((path, buf)) = self.diff_buffer.as_mut() {
            if looks_like_diff_line(line) {
                buf.push_str(line);
                buf.push('\n');
                return events;
            }
            if !buf.is_empty() {
                events.push(emit_state_delta(
                    ctx,
                    Some(path.clone()),
                    Some(buf.clone()),
                    None,
                ));
            }
            self.diff_buffer = None;
            // Fall through to parse the current line as a normal line.
        }

        // Tool/action invocations such as:
        //   ▸ bash: cargo test
        //   ● file_read: src/main.rs
        //   edit: src/lib.rs
        if let Some(cap) = tool_regex().captures(trimmed) {
            let tool = cap[1].to_lowercase();
            let target = cap[2].to_string();
            let has_marker = trimmed.starts_with('▸')
                || trimmed.starts_with('●')
                || trimmed.starts_with('›')
                || trimmed.starts_with('>');

            if has_marker || KNOWN_TOOLS.contains(&tool.as_str()) {
                events.push(emit_action_invoked(
                    ctx,
                    tool.clone(),
                    Some(target.clone()),
                    None,
                ));

                if tool == "edit" || tool == "write" || tool == "file_edit" {
                    self.diff_buffer = Some((target, String::new()));
                }
                return events;
            }
        }

        // Error / retry / timeout signals take priority over routing heuristics.
        if let Some(kind) = classify_error(trimmed) {
            events.push(emit_error_or_retry(ctx, kind, trimmed.to_string(), None));
            return events;
        }

        // Model routing decisions, e.g. "Thinking with groq", "executor: ollama".
        if let Some(cap) = routing_regex().captures(trimmed) {
            let stage = cap[1].to_lowercase();
            let model = cap[2].to_lowercase();
            events.push(emit_model_routed(ctx, stage, model, None));
            return events;
        }

        // Anything else is a response chunk.
        if !trimmed.is_empty() {
            events.push(emit_response_chunk(ctx, line.to_string()));
        }

        events
    }

    fn finalize(&mut self, ctx: &InterceptorContext, exit_code: Option<i32>) -> Vec<ScotiaEvent> {
        let mut events = Vec::new();
        if let Some((path, buf)) = self.diff_buffer.take()
            && !buf.is_empty()
        {
            events.push(emit_state_delta(ctx, Some(path), Some(buf), None));
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

fn tool_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(?i)(?:[▸●›>]\s*)?(\w+)\s*[:：]\s*(.+)$").unwrap())
}

fn routing_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Matches forms like:
        //   Thinking with groq
        //   executor: ollama
        //   model: openai
        //   Routing to local
        Regex::new(r"(?i)(?:\b|^)(thinking|planner|executor|router|routing|model|using)(?:\s*[:：]\s*|\s+(?:with|via|to)\s+)([a-z0-9_-]+)\b")
            .unwrap()
    })
}

fn looks_like_diff_line(line: &str) -> bool {
    line.starts_with("---")
        || line.starts_with("+++")
        || line.starts_with("@@")
        || line.starts_with('+')
        || line.starts_with('-')
        || (line.starts_with(' ') && !line.trim().is_empty())
}

fn classify_error(line: &str) -> Option<ErrorKind> {
    let lower = line.to_lowercase();
    if lower.starts_with("retry") || lower.contains("retrying") {
        Some(ErrorKind::Retry)
    } else if lower.starts_with("timeout") || lower.contains("timed out") {
        Some(ErrorKind::Timeout)
    } else if lower.starts_with("error") || lower.starts_with("failed") {
        if lower.contains("model") {
            Some(ErrorKind::ModelError)
        } else {
            Some(ErrorKind::ToolError)
        }
    } else {
        None
    }
}
