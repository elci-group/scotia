use crate::event::{ActionStatus, ScotiaEvent, ScotiaRun};
use std::collections::HashMap;
use uuid::Uuid;

/// Scotia Event Algebra.
///
/// The algebra treats a run as a partially ordered set of state transitions.
/// It provides operations to validate, graph, diff, and derive assertions from
/// runs without relying on full transcripts.

/// A validation issue found in a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationIssue {
    MissingRunStarted,
    MissingRunFinished,
    MultipleRunStarted,
    MultipleRunFinished,
    UnmatchedActionInvoked {
        event_id: Uuid,
        tool: String,
    },
    OrphanedActionResult {
        event_id: Uuid,
    },
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

    if let (Some(s), Some(f)) = (started.first(), finished.first()) {
        if f.timestamp() < s.timestamp() {
            issues.push(ValidationIssue::RunFinishedBeforeStarted);
        }
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

/// A single action node in the reconstructed action graph.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionNode {
    pub invocation_event_id: Uuid,
    pub tool: String,
    pub target: Option<String>,
    pub arguments: Option<serde_json::Value>,
    pub result: Option<ActionResultNode>,
    pub state_deltas: Vec<StateDeltaNode>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActionResultNode {
    pub result_event_id: Uuid,
    pub status: Option<ActionStatus>,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StateDeltaNode {
    pub path: Option<String>,
    pub description: Option<String>,
}

/// Reconstruct an action graph from a run.
pub fn action_graph(run: &ScotiaRun) -> Vec<ActionNode> {
    let mut nodes: Vec<ActionNode> = Vec::new();
    let mut open: Vec<(usize, String)> = Vec::new();

    for event in &run.events {
        match event {
            ScotiaEvent::ActionInvoked {
                event_id,
                tool,
                target,
                arguments,
                ..
            } => {
                nodes.push(ActionNode {
                    invocation_event_id: *event_id,
                    tool: tool.clone(),
                    target: target.clone(),
                    arguments: arguments.clone(),
                    result: None,
                    state_deltas: Vec::new(),
                });
                open.push((nodes.len() - 1, tool.clone()));
            }
            ScotiaEvent::ActionResult {
                event_id,
                status,
                exit_code,
                ..
            } => {
                if let Some((idx, _)) = open.pop() {
                    nodes[idx].result = Some(ActionResultNode {
                        result_event_id: *event_id,
                        status: *status,
                        exit_code: *exit_code,
                    });
                }
            }
            ScotiaEvent::StateDelta { path, description, .. } => {
                if let Some((idx, _)) = open.last() {
                    nodes[*idx].state_deltas.push(StateDeltaNode {
                        path: path.clone(),
                        description: description.clone(),
                    });
                }
            }
            _ => {}
        }
    }

    nodes
}

/// Compute a structural diff between two runs.
#[derive(Debug, Clone, PartialEq)]
pub struct RunDiff {
    pub actions_added: Vec<String>,
    pub actions_removed: Vec<String>,
    pub models_added: Vec<(String, String)>, // (stage, model)
    pub models_removed: Vec<(String, String)>,
    pub errors_added: usize,
    pub errors_removed: usize,
}

pub fn diff_runs(left: &ScotiaRun, right: &ScotiaRun) -> RunDiff {
    let left_actions = action_fingerprints(left);
    let right_actions = action_fingerprints(right);
    let left_models = model_routes(left);
    let right_models = model_routes(right);

    let actions_added: Vec<_> = right_actions
        .iter()
        .filter(|a| !left_actions.contains(a))
        .cloned()
        .collect();
    let actions_removed: Vec<_> = left_actions
        .iter()
        .filter(|a| !right_actions.contains(a))
        .cloned()
        .collect();

    let models_added: Vec<_> = right_models
        .iter()
        .filter(|m| !left_models.contains(m))
        .cloned()
        .collect();
    let models_removed: Vec<_> = left_models
        .iter()
        .filter(|m| !right_models.contains(m))
        .cloned()
        .collect();

    let left_errors = error_count(left);
    let right_errors = error_count(right);

    RunDiff {
        actions_added,
        actions_removed,
        models_added,
        models_removed,
        errors_added: right_errors.saturating_sub(left_errors),
        errors_removed: left_errors.saturating_sub(right_errors),
    }
}

fn action_fingerprints(run: &ScotiaRun) -> Vec<String> {
    run.events
        .iter()
        .filter_map(|e| match e {
            ScotiaEvent::ActionInvoked { tool, target, .. } => {
                Some(format!("{}:{}", tool, target.as_deref().unwrap_or("")))
            }
            _ => None,
        })
        .collect()
}

fn model_routes(run: &ScotiaRun) -> Vec<(String, String)> {
    run.events
        .iter()
        .filter_map(|e| match e {
            ScotiaEvent::ModelRouted { stage, model, .. } => Some((stage.clone(), model.clone())),
            _ => None,
        })
        .collect()
}

fn error_count(run: &ScotiaRun) -> usize {
    run.events
        .iter()
        .filter(|e| matches!(e, ScotiaEvent::ErrorOrRetry { .. }))
        .count()
}

/// A regression assertion derived from a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assertion {
    /// Expect at least `count` invocations of `tool`.
    ToolUsed { tool: String, count: usize },
    /// Expect the agent to route `stage` to `model`.
    ModelRouted { stage: String, model: String },
    /// Expect no error/retry events.
    NoErrors,
    /// Expect a state change at `path`.
    StateChanged { path: String },
    /// Expect a specific action sequence (by tool fingerprint).
    ActionSequence { sequence: Vec<String> },
}

/// Generate a regression suite from a run.
pub fn regression_suite(run: &ScotiaRun) -> Vec<Assertion> {
    let mut assertions = Vec::new();

    let tool_counts = run.events.iter().fold(HashMap::new(), |mut acc, e| {
        if let ScotiaEvent::ActionInvoked { tool, .. } = e {
            *acc.entry(tool.clone()).or_insert(0usize) += 1;
        }
        acc
    });

    for (tool, count) in tool_counts {
        assertions.push(Assertion::ToolUsed { tool, count });
    }

    for (stage, model) in model_routes(run) {
        assertions.push(Assertion::ModelRouted { stage, model });
    }

    if error_count(run) == 0 {
        assertions.push(Assertion::NoErrors);
    }

    let changed_paths: Vec<_> = run
        .events
        .iter()
        .filter_map(|e| match e {
            ScotiaEvent::StateDelta { path, .. } => path.clone(),
            _ => None,
        })
        .collect();
    for path in changed_paths {
        assertions.push(Assertion::StateChanged { path });
    }

    let sequence: Vec<_> = run
        .events
        .iter()
        .filter_map(|e| match e {
            ScotiaEvent::ActionInvoked { tool, .. } => Some(tool.clone()),
            _ => None,
        })
        .collect();
    if !sequence.is_empty() {
        assertions.push(Assertion::ActionSequence { sequence });
    }

    assertions
}

/// Render a regression suite as a JSON test spec.
pub fn render_regression_suite(suite: &[Assertion]) -> String {
    let value: Vec<_> = suite
        .iter()
        .map(|a| match a {
            Assertion::ToolUsed { tool, count } => serde_json::json!({
                "kind": "tool_used",
                "tool": tool,
                "count": count,
            }),
            Assertion::ModelRouted { stage, model } => serde_json::json!({
                "kind": "model_routed",
                "stage": stage,
                "model": model,
            }),
            Assertion::NoErrors => serde_json::json!({ "kind": "no_errors" }),
            Assertion::StateChanged { path } => serde_json::json!({
                "kind": "state_changed",
                "path": path,
            }),
            Assertion::ActionSequence { sequence } => serde_json::json!({
                "kind": "action_sequence",
                "sequence": sequence,
            }),
        })
        .collect();
    serde_json::to_string_pretty(&value).unwrap_or_default()
}
