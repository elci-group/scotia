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
    let mut builder = ActionGraphBuilder::default();
    for event in &run.events {
        builder.apply(event);
    }
    builder.nodes
}

#[derive(Default)]
struct ActionGraphBuilder {
    nodes: Vec<ActionNode>,
    open: Vec<(usize, String)>,
}

impl ActionGraphBuilder {
    fn apply(&mut self, event: &ScotiaEvent) {
        match event {
            ScotiaEvent::ActionInvoked {
                event_id,
                tool,
                target,
                arguments,
                ..
            } => self.on_invoked(*event_id, tool, target.as_ref(), arguments.as_ref()),
            ScotiaEvent::ActionResult {
                event_id,
                status,
                exit_code,
                ..
            } => self.on_result(*event_id, *status, *exit_code),
            ScotiaEvent::StateDelta {
                path, description, ..
            } => self.on_delta(path.as_ref(), description.as_ref()),
            _ => {}
        }
    }

    fn on_invoked(
        &mut self,
        event_id: Uuid,
        tool: &str,
        target: Option<&String>,
        arguments: Option<&serde_json::Value>,
    ) {
        self.nodes.push(ActionNode {
            invocation_event_id: event_id,
            tool: tool.to_string(),
            target: target.cloned(),
            arguments: arguments.cloned(),
            result: None,
            state_deltas: Vec::new(),
        });
        self.open.push((self.nodes.len() - 1, tool.to_string()));
    }

    fn on_result(&mut self, event_id: Uuid, status: Option<ActionStatus>, exit_code: Option<i32>) {
        if let Some((idx, _)) = self.open.pop() {
            self.nodes[idx].result = Some(ActionResultNode {
                result_event_id: event_id,
                status,
                exit_code,
            });
        }
    }

    fn on_delta(&mut self, path: Option<&String>, description: Option<&String>) {
        if let Some((idx, _)) = self.open.last() {
            self.nodes[*idx].state_deltas.push(StateDeltaNode {
                path: path.cloned(),
                description: description.cloned(),
            });
        }
    }
}
