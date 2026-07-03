use super::*;
use crate::adapter::{AdapterContext, AgentAdapter, StreamSource};
use regex::Regex;

#[derive(Default)]
pub struct KimiAdapter;

impl AgentAdapter for KimiAdapter {
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

        // Kimi Code may prefix tool calls with markers like "● file_read" or "▸ bash"
        if let Some(cap) = Regex::new(r"^(?:[▸●›>]\s*)?(\w+)\s*[:：]\s*(.+)$")
            .unwrap()
            .captures(trimmed)
        {
            let tool = cap[1].to_lowercase();
            let target = cap[2].to_string();
            events.push(emit_action_invoked(ctx, tool, Some(target), None));
            return events;
        }

        // Routing hints such as "Thinking with groq" or "executor: ollama".
        if let Some(cap) = Regex::new(r"(?i)(?:thinking|planner|executor|router):?\s+([a-z0-9_-]+)")
            .unwrap()
            .captures(trimmed)
        {
            events.push(ScotiaEvent::ModelRouted {
                event_id: crate::adapter::new_event_id(),
                run_id: ctx.run_id,
                timestamp: chrono::Utc::now(),
                stage: "inference".to_string(),
                model: cap[1].to_string(),
                latency_ms: None,
                metadata: Default::default(),
            });
        }

        if trimmed.to_lowercase().contains("retry") || trimmed.to_lowercase().contains("timeout") {
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
