use super::*;
use crate::event::{ErrorKind, ScotiaEvent};
use crate::interceptor::{AgentInterceptor, InterceptorContext, StreamSource};
use regex::Regex;

/// Cosine agent interceptor.
///
/// Parses the structured telemetry Cosine emits on stdout/stderr into canonical
/// Scotia events: tool invocations, model routing decisions, file edits/diffs,
/// error/retry signals, and free-form response chunks.
#[derive(Default)]
pub struct CosineInterceptor;

impl AgentInterceptor for CosineInterceptor {
    fn name(&self) -> &'static str {
        "cosine"
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

        // 1. Model routing decision (planner -> groq, etc.).
        if let Some(event) = try_parse_model_routed(ctx, trimmed) {
            return vec![event];
        }

        // 2. Tool/action invocation.
        if let Some(event) = try_parse_action_invoked(ctx, trimmed) {
            return vec![event];
        }

        // 3. File edit / diff block.
        if let Some(event) = try_parse_state_delta(ctx, trimmed) {
            return vec![event];
        }

        // 4. Explicit error or retry signal on stdout.
        if let Some(event) = try_parse_error_retry(ctx, trimmed) {
            return vec![event];
        }

        // 5. Plain response chunk.
        if !trimmed.is_empty() {
            return vec![emit_response_chunk(ctx, line.to_string())];
        }

        Vec::new()
    }
}

fn try_parse_model_routed(ctx: &InterceptorContext, line: &str) -> Option<ScotiaEvent> {
    // Handles forms like:
    //   MODEL planner=groq
    //   MODEL planner -> groq
    //   USING_MODEL planner groq
    //   ROUTE to planner groq
    let re = Regex::new(
        r"(?i)^(?:MODEL|USING_MODEL|ROUTE)\s+(?:to\s+)?([\w_-]+)(?:\s*(?:=|->)\s*|\s+)([\w_.:-]+)$",
    )
    .unwrap();
    let cap = re.captures(line)?;
    let stage = cap[1].to_string();
    let model = cap[2].to_string();
    Some(emit_model_routed(ctx, stage, model, None))
}

fn try_parse_action_invoked(ctx: &InterceptorContext, line: &str) -> Option<ScotiaEvent> {
    // Cosine emits lines like: "ACTION read_file path=src/main.rs"
    let re = Regex::new(r"(?i)^ACTION\s+(\w+)\s*(.*)$").unwrap();
    let cap = re.captures(line)?;
    let tool = cap[1].to_lowercase();
    let rest = cap[2].to_string();
    let target = parse_target(&rest);
    let arguments = if rest.is_empty() {
        None
    } else {
        Some(serde_json::Value::String(rest))
    };
    Some(emit_action_invoked(ctx, tool, target, arguments))
}

fn parse_target(rest: &str) -> Option<String> {
    rest.split_whitespace()
        .find(|t| t.contains('='))
        .and_then(|t| t.split_once('=').map(|(_, v)| v.to_string()))
}

fn try_parse_state_delta(ctx: &InterceptorContext, line: &str) -> Option<ScotiaEvent> {
    // Explicit edit/diff/patch annotation with path.
    let explicit = Regex::new(r"(?i)^(EDIT|DIFF|PATCH)\s+path=(\S+)(?:\s+(.*))?$").unwrap();
    if let Some(cap) = explicit.captures(line) {
        let path = cap[2].to_string();
        let description = cap.get(3).map(|m| m.as_str().to_string());
        return Some(emit_state_delta(ctx, Some(path), None, description));
    }

    // Unified diff header: "--- a/src/main.rs" / "+++ b/src/main.rs"
    if line.starts_with("--- ") || line.starts_with("+++ ") {
        let path = line[4..].split_whitespace().next().map(|s| {
            s.strip_prefix("a/")
                .or_else(|| s.strip_prefix("b/"))
                .unwrap_or(s)
                .to_string()
        });
        return Some(emit_state_delta(ctx, path, Some(line.to_string()), None));
    }

    // Hunk header or change lines.
    if line.starts_with("@@ ") || line.starts_with("+ ") || line.starts_with("- ") {
        return Some(emit_state_delta(ctx, None, Some(line.to_string()), None));
    }

    None
}

fn try_parse_error_retry(ctx: &InterceptorContext, line: &str) -> Option<ScotiaEvent> {
    let re = Regex::new(r"(?i)^(?:ERROR|ERR|FAILED|FAIL|RETRY)\s*[:=-]?\s*(.*)$").unwrap();
    let cap = re.captures(line)?;
    let message = cap[1].to_string();
    let kind = if line.to_uppercase().starts_with("RETRY") {
        ErrorKind::Retry
    } else {
        ErrorKind::ToolError
    };
    Some(emit_error_or_retry(ctx, kind, message, None))
}
