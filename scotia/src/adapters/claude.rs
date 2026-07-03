use super::*;
use crate::adapter::{AdapterContext, AgentAdapter, StreamSource};
use crate::event::ScotiaEvent;
use regex::Regex;

#[derive(Default)]
pub struct ClaudeAdapter {
    /// Accumulated multi-line diff block.
    diff_buffer: Option<(String, String)>, // (path, content)
}

impl AgentAdapter for ClaudeAdapter {
    fn parse_line(
        &mut self,
        ctx: &AdapterContext,
        source: StreamSource,
        line: &str,
    ) -> Vec<ScotiaEvent> {
        if source == StreamSource::Stderr {
            return classify_stderr(ctx, line).into_iter().collect();
        }

        let mut events = Vec::new();
        let trimmed = line.trim();

        // Claude Code often prints tool use in forms like:
        //   › Search: grep -r "foo" src/
        //   › Read: src/main.rs
        //   › Edit: src/main.rs
        //   › Bash: cargo test
        if let Some(cap) = Regex::new(r"^[›>]\s*(\w+):\s*(.+)$")
            .unwrap()
            .captures(trimmed)
        {
            let tool = cap[1].to_lowercase();
            let rest = cap[2].to_string();
            events.push(emit_action_invoked(
                ctx,
                tool.clone(),
                Some(rest.clone()),
                None,
            ));

            // If it's an edit, start accumulating a diff.
            if tool == "edit" || tool == "write" {
                self.diff_buffer = Some((rest, String::new()));
            }
            return events;
        }

        // Detect file edit diff blocks (Claude Code often prints unified diffs).
        if let Some((path, buf)) = self.diff_buffer.as_mut() {
            if trimmed.starts_with("---")
                || trimmed.starts_with("+++")
                || trimmed.starts_with("@@")
                || trimmed.starts_with('+')
                || trimmed.starts_with('-')
                || trimmed.starts_with(' ')
            {
                buf.push_str(line);
                buf.push('\n');
            } else if buf.len() > 40 {
                events.push(emit_state_delta(
                    ctx,
                    Some(path.clone()),
                    Some(buf.clone()),
                    None,
                ));
                self.diff_buffer = None;
            }
        }

        // Routing hints: "Using model: ..." or "Routing to groq"
        if let Some(cap) = Regex::new(r"(?i)(?:using|routed to|model:\s*)([a-z0-9_-]+)")
            .unwrap()
            .captures(trimmed)
        {
            let model = cap[1].to_string();
            if model != "claude" {
                events.push(ScotiaEvent::ModelRouted {
                    event_id: crate::adapter::new_event_id(),
                    run_id: ctx.run_id,
                    timestamp: chrono::Utc::now(),
                    stage: "unknown".to_string(),
                    model,
                    latency_ms: None,
                    metadata: Default::default(),
                });
            }
        }

        // Retry / error signals.
        if trimmed.to_lowercase().contains("retrying")
            || trimmed.to_lowercase().contains("try again")
        {
            events.push(emit_error_or_retry(
                ctx,
                crate::event::ErrorKind::Retry,
                trimmed.to_string(),
                None,
            ));
            return events;
        }

        // Otherwise treat as response output.
        if !trimmed.is_empty() {
            events.push(emit_response_chunk(ctx, line.to_string()));
        }

        events
    }
}
