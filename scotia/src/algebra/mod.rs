//! Scotia Event Algebra.
//!
//! The algebra treats a run as a partially ordered set of state transitions.
//! It provides operations to validate, graph, diff, and derive assertions from
//! runs without relying on full transcripts.
//!
//! Split into cohesive submodules; the public surface (`algebra::validate`,
//! `algebra::action_graph`, `algebra::diff_runs`, `algebra::regression_suite`,
//! `algebra::render_regression_suite`, and their types) is re-exported here so
//! callers are unaffected.

mod diff;
mod graph;
mod regression;
mod validate;

pub use diff::{RunDiff, diff_runs};
pub use graph::{ActionNode, ActionResultNode, StateDeltaNode, action_graph};
pub use regression::{Assertion, regression_suite, render_regression_suite};
pub use validate::{ValidationIssue, validate};

use crate::event::{ScotiaEvent, ScotiaRun};

/// `(stage, model)` routing decisions in a run, in emission order. Shared by
/// the diff and regression passes.
pub(crate) fn model_routes(run: &ScotiaRun) -> Vec<(String, String)> {
    run.events
        .iter()
        .filter_map(|e| match e {
            ScotiaEvent::ModelRouted { stage, model, .. } => Some((stage.clone(), model.clone())),
            _ => None,
        })
        .collect()
}

/// Count of error/retry events in a run. Shared by the diff and regression passes.
pub(crate) fn error_count(run: &ScotiaRun) -> usize {
    run.events
        .iter()
        .filter(|e| matches!(e, ScotiaEvent::ErrorOrRetry { .. }))
        .count()
}
