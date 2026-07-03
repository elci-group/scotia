use super::*;
use crate::adapter::{AdapterContext, AgentAdapter, StreamSource};
use regex::Regex;

#[derive(Default)]
pub struct CosineAdapter;

impl AgentAdapter for CosineAdapter {
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

        // Cosine emits lines like: "ACTION read_file path=src/main.rs"
        if let Some(cap) = Regex::new(r"(?i)^ACTION\s+(\w+)\s*(.*)$")
            .unwrap()
            .captures(trimmed)
        {
            let tool = cap[1].to_lowercase();
            let rest = cap[2].to_string();
            let target = rest
                .split_whitespace()
                .find(|t| t.contains('='))
                .map(|t| t.split_once('=').map(|(_, v)| v.to_string()))
                .flatten();
            let arguments = if rest.is_empty() {
                None
            } else {
                Some(serde_json::Value::String(rest))
            };
            events.push(emit_action_invoked(ctx, tool, target, arguments));
            return events;
        }

        if !trimmed.is_empty() {
            events.push(emit_response_chunk(ctx, line.to_string()));
        }
        events
    }
}
