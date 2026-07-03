use super::*;
use crate::adapter::{AdapterContext, AgentAdapter, StreamSource};
use regex::Regex;

#[derive(Default)]
pub struct CodexAdapter;

impl AgentAdapter for CodexAdapter {
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

        // Codex CLI often prints: "[edit] src/main.rs" or "[bash] cargo test"
        if let Some(cap) = Regex::new(r"^\[(\w+)\]\s*(.+)$").unwrap().captures(trimmed) {
            let tool = cap[1].to_lowercase();
            let target = cap[2].to_string();
            events.push(emit_action_invoked(ctx, tool, Some(target), None));
            return events;
        }

        // Model routing hints.
        if let Some(cap) = Regex::new(r"(?i)(?:model|routing|using):\s*([a-z0-9_-]+)")
            .unwrap()
            .captures(trimmed)
        {
            events.push(ScotiaEvent::ModelRouted {
                event_id: crate::adapter::new_event_id(),
                run_id: ctx.run_id,
                timestamp: chrono::Utc::now(),
                stage: "generation".to_string(),
                model: cap[1].to_string(),
                latency_ms: None,
                metadata: Default::default(),
            });
        }

        if trimmed.to_lowercase().contains("retry") {
            events.push(emit_error_or_retry(
                ctx,
                crate::event::ErrorKind::Retry,
                trimmed.to_string(),
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
