use super::*;
use crate::adapter::{AdapterContext, AgentAdapter, StreamSource};
use regex::Regex;

#[derive(Default)]
pub struct AgyAdapter;

impl AgentAdapter for AgyAdapter {
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

        // agy tends to emit structured JSON lines for tool calls.
        if trimmed.starts_with('{')
            && trimmed.ends_with('}')
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

        if let Some(cap) = Regex::new(r"(?i)(?:tool|action):\s*(\w+)")
            .unwrap()
            .captures(trimmed)
        {
            events.push(emit_action_invoked(ctx, &cap[1], None, None));
            return events;
        }

        if !trimmed.is_empty() {
            events.push(emit_response_chunk(ctx, line.to_string()));
        }
        events
    }
}
