#![allow(dead_code)]

use chrono::Utc;
use scotia::event::{
    ActionStatus, AgentKind, ErrorKind, Role, ScotiaEvent, ScotiaRun,
};
use uuid::Uuid;

/// Create a minimal run with a RunStarted event already present.
pub fn empty_run(agent: AgentKind) -> ScotiaRun {
    ScotiaRun::new(agent, Some("stress test".to_string()))
}

/// Add an action invocation to a run.
pub fn action_invoked(run: &mut ScotiaRun, tool: &str, target: Option<&str>) -> Uuid {
    let id = Uuid::new_v4();
    run.push(ScotiaEvent::ActionInvoked {
        event_id: id,
        run_id: run.run_id,
        timestamp: Utc::now(),
        tool: tool.to_string(),
        target: target.map(|s| s.to_string()),
        arguments: None,
    });
    id
}

/// Add a matching action result to a run.
pub fn action_result(run: &mut ScotiaRun, status: ActionStatus, exit_code: i32) -> Uuid {
    let id = Uuid::new_v4();
    run.push(ScotiaEvent::ActionResult {
        event_id: id,
        run_id: run.run_id,
        timestamp: Utc::now(),
        status: Some(status),
        stdout: None,
        stderr: None,
        exit_code: Some(exit_code),
    });
    id
}

/// Add a model routing event.
pub fn model_routed(run: &mut ScotiaRun, stage: &str, model: &str) {
    run.push(ScotiaEvent::ModelRouted {
        event_id: Uuid::new_v4(),
        run_id: run.run_id,
        timestamp: Utc::now(),
        stage: stage.to_string(),
        model: model.to_string(),
        latency_ms: None,
        metadata: Default::default(),
    });
}

/// Add a response chunk.
pub fn response_chunk(run: &mut ScotiaRun, content: &str) {
    run.push(ScotiaEvent::ResponseChunk {
        event_id: Uuid::new_v4(),
        run_id: run.run_id,
        timestamp: Utc::now(),
        content: content.to_string(),
        finish_reason: None,
    });
}

/// Add an error/retry event.
pub fn error_or_retry(run: &mut ScotiaRun, kind: ErrorKind, message: &str) {
    run.push(ScotiaEvent::ErrorOrRetry {
        event_id: Uuid::new_v4(),
        run_id: run.run_id,
        timestamp: Utc::now(),
        kind,
        message: message.to_string(),
        retry_count: None,
    });
}

/// Add a state delta.
pub fn state_delta(run: &mut ScotiaRun, path: &str, description: &str) {
    run.push(ScotiaEvent::StateDelta {
        event_id: Uuid::new_v4(),
        run_id: run.run_id,
        timestamp: Utc::now(),
        path: Some(path.to_string()),
        diff: None,
        description: Some(description.to_string()),
    });
}

/// Add a prompt submission.
pub fn prompt(run: &mut ScotiaRun, role: Role, content: &str) {
    run.push(ScotiaEvent::PromptSubmitted {
        event_id: Uuid::new_v4(),
        run_id: run.run_id,
        timestamp: Utc::now(),
        role,
        content: content.to_string(),
        context: Default::default(),
    });
}

/// Finish a run with the given exit code.
pub fn finish(run: &mut ScotiaRun, exit_code: i32) {
    run.finish(Some(exit_code), None);
}

/// Generate a large chunk of random-ish text.
pub fn big_text(words: usize) -> String {
    (0..words)
        .map(|i| format!("word{}", i))
        .collect::<Vec<_>>()
        .join(" ")
}
