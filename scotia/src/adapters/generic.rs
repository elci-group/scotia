use super::*;
use crate::adapter::{AdapterContext, AgentAdapter, StreamSource};
use regex::Regex;

#[derive(Default)]
pub struct GenericAdapter;

impl AgentAdapter for GenericAdapter {
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

        // Generic heuristics for unknown agents.
        if let Some(cap) = Regex::new(r"(?i)(?:tool|action|call|exec)[\s:\]]+([a-z_]+)")
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
