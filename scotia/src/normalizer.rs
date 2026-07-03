use crate::event::{ScotiaEvent, ScotiaRun};
use std::collections::HashMap;

/// Normalize a raw captured run into a canonical form.
///
/// This pass:
///  - sorts events by timestamp,
///  - coalesces adjacent response chunks into larger semantic blocks,
///  - keeps only the first run-started and last run-finished markers.
pub fn normalize(mut run: ScotiaRun) -> ScotiaRun {
    run.events.sort_by_key(|e| e.timestamp());

    let mut normalized: Vec<ScotiaEvent> = Vec::with_capacity(run.events.len());
    let mut chunk_buffer: Option<ScotiaEvent> = None;

    for event in run.events {
        match event {
            ScotiaEvent::ResponseChunk {
                event_id,
                run_id,
                timestamp,
                content,
                finish_reason,
            } => {
                if let Some(ScotiaEvent::ResponseChunk {
                    content: ref mut buf,
                    finish_reason: ref mut buf_fr,
                    ..
                }) = chunk_buffer
                {
                    buf.push('\n');
                    buf.push_str(&content);
                    *buf_fr = finish_reason.clone();
                } else {
                    chunk_buffer = Some(ScotiaEvent::ResponseChunk {
                        event_id,
                        run_id,
                        timestamp,
                        content,
                        finish_reason,
                    });
                }
            }
            ScotiaEvent::ActionInvoked { .. } => {
                flush_chunk(&mut normalized, &mut chunk_buffer);
                normalized.push(event);
            }
            ScotiaEvent::ActionResult { .. } => {
                flush_chunk(&mut normalized, &mut chunk_buffer);
                normalized.push(event);
            }
            ScotiaEvent::RunStarted { .. } => {
                if !normalized
                    .iter()
                    .any(|e| matches!(e, ScotiaEvent::RunStarted { .. }))
                {
                    flush_chunk(&mut normalized, &mut chunk_buffer);
                    normalized.push(event);
                }
            }
            ScotiaEvent::RunFinished { .. } => {
                flush_chunk(&mut normalized, &mut chunk_buffer);
                normalized.retain(|e| !matches!(e, ScotiaEvent::RunFinished { .. }));
                normalized.push(event);
            }
            _ => {
                flush_chunk(&mut normalized, &mut chunk_buffer);
                normalized.push(event);
            }
        }
    }

    flush_chunk(&mut normalized, &mut chunk_buffer);
    run.events = normalized;
    run.metadata = compute_metadata(&run);
    run
}

fn flush_chunk(normalized: &mut Vec<ScotiaEvent>, chunk_buffer: &mut Option<ScotiaEvent>) {
    if let Some(ScotiaEvent::ResponseChunk {
        content,
        event_id,
        run_id,
        timestamp,
        finish_reason,
    }) = chunk_buffer.take()
    {
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            normalized.push(ScotiaEvent::ResponseChunk {
                event_id,
                run_id,
                timestamp,
                content: trimmed.to_string(),
                finish_reason,
            });
        }
    }
}

fn compute_metadata(run: &ScotiaRun) -> HashMap<String, serde_json::Value> {
    let mut meta = run.metadata.clone();
    let mut action_count = 0u64;
    let mut error_count = 0u64;
    let mut model_routes = Vec::new();

    for event in &run.events {
        match event {
            ScotiaEvent::ActionInvoked { .. } => action_count += 1,
            ScotiaEvent::ErrorOrRetry { .. } => error_count += 1,
            ScotiaEvent::ModelRouted { model, stage, .. } => {
                model_routes.push(serde_json::json!({
                    "stage": stage,
                    "model": model,
                }));
            }
            _ => {}
        }
    }

    meta.insert("action_count".to_string(), action_count.into());
    meta.insert("error_count".to_string(), error_count.into());
    meta.insert("model_routes".to_string(), model_routes.into());
    meta
}
