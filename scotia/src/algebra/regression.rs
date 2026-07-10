//! Regression assertion derivation and rendering.

use std::collections::HashMap;

use crate::event::{ScotiaEvent, ScotiaRun};

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

    for (stage, model) in super::model_routes(run) {
        assertions.push(Assertion::ModelRouted { stage, model });
    }

    if super::error_count(run) == 0 {
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
