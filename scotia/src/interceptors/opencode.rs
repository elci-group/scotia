use crate::event::{ActionStatus, ErrorKind, ScotiaEvent};
use crate::interceptor::{AgentInterceptor, InterceptorContext, StreamSource};
use crate::interceptors::{
    classify_stderr, emit_action_invoked, emit_action_result, emit_error_or_retry,
    emit_model_routed, emit_response_chunk, emit_state_delta,
};
use regex::Regex;

/// Parser for opencode agent telemetry.
///
/// opencode surfaces tool calls, routing decisions, edits and errors through
/// a small set of prefixed markers such as `[TOOL]`, `[MODEL]`, `[ERROR]` and
/// `[RETRY]`.  The interceptor also accumulates unified-diff style blocks that
/// follow `edit` or `write` tool calls.
#[derive(Default)]
pub struct OpencodeInterceptor {
    /// Active diff accumulation: (file_path, accumulated_diff_lines).
    diff_buffer: Option<(String, String)>,
}

impl AgentInterceptor for OpencodeInterceptor {
    fn name(&self) -> &'static str {
        "opencode"
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

        // Finish an active diff block before other processing; empty lines terminate diffs.
        if let Some((path, buf)) = self.diff_buffer.as_mut() {
            if trimmed.is_empty() || !is_diff_line(trimmed) {
                if !buf.is_empty() {
                    events.push(emit_state_delta(
                        ctx,
                        Some(path.clone()),
                        Some(buf.clone()),
                        None,
                    ));
                }
                self.diff_buffer = None;
                if trimmed.is_empty() {
                    return events;
                }
            } else {
                buf.push_str(line);
                buf.push('\n');
                return events;
            }
        }

        if trimmed.is_empty() {
            return events;
        }

        // Structured side-channel JSON payloads (e.g. MCP messages).
        if source == StreamSource::SideChannel
            && trimmed.starts_with('{')
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed)
            && let Some(tool) = value.get("tool").and_then(|v| v.as_str())
        {
            events.push(emit_action_invoked(
                ctx,
                tool,
                value
                    .get("target")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                value.get("arguments").cloned(),
            ));
            return events;
        }

        // Explicit retry markers: "[RETRY] [2] connection timed out".
        if let Some(cap) = Regex::new(r"(?i)^\[RETRY\](?:\s*\[(\d+)\])?\s*(.+)$")
            .unwrap()
            .captures(trimmed)
        {
            let retry_count = cap.get(1).and_then(|m| m.as_str().parse::<u32>().ok());
            events.push(emit_error_or_retry(
                ctx,
                ErrorKind::Retry,
                cap[2].to_string(),
                retry_count,
            ));
            return events;
        }

        // Explicit error markers.
        if let Some(cap) = Regex::new(r"(?i)^\[ERROR\]\s*(.+)$")
            .unwrap()
            .captures(trimmed)
        {
            events.push(emit_error_or_retry(
                ctx,
                ErrorKind::ToolError,
                cap[1].to_string(),
                None,
            ));
            return events;
        }

        // Model routing decisions.
        //
        // Supported forms:
        //   [MODEL] planner: groq
        //   planner -> groq
        //   model: ollama
        //   Routing to openai
        //   Using model: local
        let routing_re =
            Regex::new(r"(?i)^(?:\[MODEL\]\s*)?(\w+)\s*(?:[:=]|->)\s*([a-z0-9_-]+)$").unwrap();
        if let Some(cap) = routing_re.captures(trimmed) {
            let stage_key = cap[1].to_lowercase();
            let known = [
                "planner",
                "executor",
                "router",
                "stage",
                "reasoning",
                "model",
            ];
            if known.contains(&stage_key.as_str()) {
                let stage = match stage_key.as_str() {
                    "planner" | "executor" | "router" | "stage" | "reasoning" => stage_key,
                    _ => "routing".to_string(),
                };
                events.push(emit_model_routed(ctx, stage, &cap[2], None));
                return events;
            }
        }

        if let Some(cap) = Regex::new(
            r"(?i)(?:using model|routed to|routing to|model|routing)\s*(?:[:=])?\s+([a-z0-9_-]+)",
        )
        .unwrap()
        .captures(trimmed)
        {
            events.push(emit_model_routed(ctx, "inference", &cap[1], None));
            return events;
        }

        // Tool / action invocations.
        //
        // Examples:
        //   [TOOL] read_file: src/main.rs
        //   [ACTION] bash: cargo test
        //   ▸ grep: pattern src/
        if let Some(cap) =
            Regex::new(r"(?i)^(?:\[TOOL\]|\[ACTION\]|[▸●›>])\s*(\w+)\s*[:：]\s*(.+)$")
                .unwrap()
                .captures(trimmed)
        {
            let tool = cap[1].to_lowercase();
            let target = cap[2].to_string();

            if is_edit_tool(&tool) {
                self.diff_buffer = Some((target.clone(), String::new()));
            }

            events.push(emit_action_invoked(ctx, tool, Some(target), None));
            return events;
        }

        // Action results: "[RESULT] status: success exit_code: 0".
        if trimmed.to_uppercase().starts_with("[RESULT]") {
            let status = Regex::new(r"(?i)status\s*[:=]\s*(\w+)")
                .unwrap()
                .captures(trimmed)
                .and_then(|c| parse_action_status(&c[1]));
            let exit_code = Regex::new(r"(?i)exit(?:_code)?\s*[:=]\s*(-?\d+)")
                .unwrap()
                .captures(trimmed)
                .and_then(|c| c[1].parse::<i32>().ok());
            events.push(emit_action_result(ctx, status, None, None, exit_code));
            return events;
        }

        // Fall-back error / retry heuristics.
        let lower = trimmed.to_lowercase();
        if lower.starts_with("retry") || lower.starts_with("try again") {
            events.push(emit_error_or_retry(
                ctx,
                ErrorKind::Retry,
                trimmed.to_string(),
                None,
            ));
            return events;
        }
        if lower.starts_with("timeout") {
            events.push(emit_error_or_retry(
                ctx,
                ErrorKind::Timeout,
                trimmed.to_string(),
                None,
            ));
            return events;
        }
        if lower.starts_with("error") || lower.starts_with("fatal") {
            events.push(emit_error_or_retry(
                ctx,
                ErrorKind::ToolError,
                trimmed.to_string(),
                None,
            ));
            return events;
        }

        // Anything unmatched is treated as response output.
        events.push(emit_response_chunk(ctx, line.to_string()));
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

fn is_edit_tool(tool: &str) -> bool {
    matches!(
        tool,
        "edit" | "write" | "apply_patch" | "patch" | "file_edit" | "replace"
    )
}

fn is_diff_line(trimmed: &str) -> bool {
    trimmed.starts_with("---")
        || trimmed.starts_with("+++")
        || trimmed.starts_with("@@")
        || trimmed.starts_with('+')
        || trimmed.starts_with('-')
        || trimmed.starts_with(' ')
}

fn parse_action_status(s: &str) -> Option<ActionStatus> {
    match s.to_lowercase().as_str() {
        "success" | "ok" | "done" => Some(ActionStatus::Success),
        "failure" | "fail" | "error" => Some(ActionStatus::Failure),
        "cancelled" | "canceled" => Some(ActionStatus::Cancelled),
        _ => None,
    }
}
