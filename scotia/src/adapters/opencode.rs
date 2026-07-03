use super::*;
use crate::adapter::{AdapterContext, AgentAdapter, StreamSource};
use regex::Regex;

#[derive(Default)]
pub struct OpencodeAdapter;

impl AgentAdapter for OpencodeAdapter {
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

        // opencode prints tool usage like: "[TOOL] read_file: src/main.rs"
        if let Some(cap) = Regex::new(r"(?i)^\[TOOL\]\s+(\w+)\s*[:：]\s*(.+)$")
            .unwrap()
            .captures(trimmed)
        {
            events.push(emit_action_invoked(
                ctx,
                &cap[1],
                Some(cap[2].to_string()),
                None,
            ));
            return events;
        }

        if !trimmed.is_empty() {
            events.push(emit_response_chunk(ctx, line.to_string()));
        }
        events
    }
}
