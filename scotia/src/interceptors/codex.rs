use super::*;
use crate::event::ScotiaEvent;
use crate::interceptor::{AgentInterceptor, InterceptorContext, StreamSource};
use regex::Regex;
use std::sync::OnceLock;

/// Interceptor for the OpenAI `codex` CLI agent.
///
/// Codex surfaces tool calls as bracketed markers (`[bash] cargo test`),
/// structured JSON function calls, and plain-text action annotations. It also
/// prints model routing hints, unified diff blocks, and retry/error messages.
/// This interceptor parses all of those into canonical Scotia events.
#[derive(Default)]
pub struct CodexInterceptor {
    /// Accumulated multi-line diff block when an edit/write is in flight.
    /// Stores `(path, accumulated_diff)`.
    diff_buffer: Option<(String, String)>,
}

impl CodexInterceptor {
    fn flush_diff(&mut self, ctx: &InterceptorContext) -> Option<ScotiaEvent> {
        self.diff_buffer.take().and_then(|(path, buf)| {
            if buf.is_empty() {
                None
            } else {
                Some(emit_state_delta(ctx, Some(path), Some(buf), None))
            }
        })
    }
}

impl AgentInterceptor for CodexInterceptor {
    fn name(&self) -> &'static str {
        "codex"
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
        if let Some(event) = take_diff_if_broken(&mut self.diff_buffer, is_diff_line(trimmed), ctx)
        {
            events.push(event);
        }

        // Bracketed tool invocations: [bash] cargo test, [read] src/main.rs
        static RE_BRACKET: OnceLock<Regex> = OnceLock::new();
        if let Some(cap) = cached_regex(&RE_BRACKET, r"^\[(\w+)\]\s*(.+)$").captures(trimmed) {
            let tool = cap[1].to_lowercase();
            let target = cap[2].to_string();

            if tool.eq_ignore_ascii_case("edit") || tool.eq_ignore_ascii_case("write") {
                self.diff_buffer = Some((target.clone(), String::new()));
            }

            events.push(emit_action_invoked(ctx, tool, Some(target), None));
            return events;
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
                .or_else(|| value.get("path"))
                .or_else(|| value.get("file_path"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let arguments = value.get("arguments").cloned();

            if tool.eq_ignore_ascii_case("edit")
                || tool.eq_ignore_ascii_case("write")
                || tool.eq_ignore_ascii_case("apply_patch")
            {
                let path = target.clone().unwrap_or_default();
                self.diff_buffer = Some((path, String::new()));
            }

            events.push(emit_action_invoked(ctx, tool, target, arguments));
            return events;
        }

        // Plain-text action annotation: "bash: cargo test" or "▸ read: src/main.rs".
        // Skip routing keywords so "model: groq" is captured as ModelRouted instead.
        static RE_ACTION: OnceLock<Regex> = OnceLock::new();
        if let Some(cap) =
            cached_regex(&RE_ACTION, r"(?i)(?:[▸●›>\-]\s*)?(\w+)\s*[:：]\s*(.+)$").captures(trimmed)
        {
            let tool = cap[1].to_lowercase();
            if !matches!(
                tool.as_str(),
                "model" | "routing" | "router" | "using" | "error" | "failed"
            ) {
                let target = cap[2].to_string();

                if tool.eq_ignore_ascii_case("edit") || tool.eq_ignore_ascii_case("write") {
                    self.diff_buffer = Some((target.clone(), String::new()));
                }

                events.push(emit_action_invoked(ctx, tool, Some(target), None));
                return events;
            }
        }

        // Model routing decision: groq, ollama, openai, local, etc.
        static RE_MODEL: OnceLock<Regex> = OnceLock::new();
        if let Some(cap) = cached_regex(&RE_MODEL, r"(?i)(?:model|routing|router)\s*[:：]\s*([a-z0-9_\.-]+)$|\b(?:using|routed to)\s+([a-z0-9_\.-]+)")
            .captures(trimmed)
        {
            let model = cap
                .get(1)
                .or_else(|| cap.get(2))
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            if !model.is_empty() {
                events.push(emit_model_routed(ctx, "generation", model, None));
                return events;
            }
        }

        // Error / retry signals.
        let lower = trimmed.to_lowercase();
        if let Some(kind) = classify_error(&lower) {
            events.push(emit_error_or_retry(ctx, kind, trimmed.to_string(), None));
            return events;
        }

        // Accumulate unified-diff blocks after an edit/write.
        if accumulate_diff(&mut self.diff_buffer, line, is_diff_line(trimmed)) {
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
        if let Some(event) = self.flush_diff(ctx) {
            events.push(event);
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
