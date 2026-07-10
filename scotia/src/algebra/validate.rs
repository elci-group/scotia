//! Structural validation of a run.

use uuid::Uuid;

use crate::event::{ScotiaEvent, ScotiaRun};

/// A validation issue found in a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationIssue {
    MissingRunStarted,
    MissingRunFinished,
    MultipleRunStarted,
    MultipleRunFinished,
    UnmatchedActionInvoked { event_id: Uuid, tool: String },
    OrphanedActionResult { event_id: Uuid },
    RunFinishedBeforeStarted,
}

/// Validate the structural correctness of a run.
pub fn validate(run: &ScotiaRun) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    let started: Vec<_> = run
        .events
        .iter()
        .filter(|e| matches!(e, ScotiaEvent::RunStarted { .. }))
        .collect();
    let finished: Vec<_> = run
        .events
        .iter()
        .filter(|e| matches!(e, ScotiaEvent::RunFinished { .. }))
        .collect();

    if started.is_empty() {
        issues.push(ValidationIssue::MissingRunStarted);
    }
    if started.len() > 1 {
        issues.push(ValidationIssue::MultipleRunStarted);
    }
    if finished.is_empty() {
        issues.push(ValidationIssue::MissingRunFinished);
    }
    if finished.len() > 1 {
        issues.push(ValidationIssue::MultipleRunFinished);
    }

    if let (Some(s), Some(f)) = (started.first(), finished.first())
        && f.timestamp() < s.timestamp()
    {
        issues.push(ValidationIssue::RunFinishedBeforeStarted);
    }

    // Match action invoked -> result using a simple stack per tool.
    let mut pending: Vec<(Uuid, String)> = Vec::new();
    for event in &run.events {
        match event {
            ScotiaEvent::ActionInvoked { event_id, tool, .. } => {
                pending.push((*event_id, tool.clone()));
            }
            ScotiaEvent::ActionResult { event_id, .. } => {
                if pending.is_empty() {
                    issues.push(ValidationIssue::OrphanedActionResult {
                        event_id: *event_id,
                    });
                } else {
                    pending.pop();
                }
            }
            _ => {}
        }
    }

    for (event_id, tool) in pending {
        issues.push(ValidationIssue::UnmatchedActionInvoked { event_id, tool });
    }

    issues
}
