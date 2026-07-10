//! Reconstruction of the action graph (invocation -> result + state deltas).

use uuid::Uuid;

use crate::event::{ActionStatus, ScotiaEvent, ScotiaRun};

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
            ScotiaEvent::StateDelta {
                path, description, ..
            } => {
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
