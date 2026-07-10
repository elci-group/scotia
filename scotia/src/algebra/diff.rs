//! Structural diff between two runs.

use crate::event::{ScotiaEvent, ScotiaRun};

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
    let left_models = super::model_routes(left);
    let right_models = super::model_routes(right);

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

    let left_errors = super::error_count(left);
    let right_errors = super::error_count(right);

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
