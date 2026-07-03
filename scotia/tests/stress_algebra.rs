//! Stress tests for the Scotia event algebra.
//!
//! Covers `validate`, `action_graph`, `diff_runs`, and `regression_suite`
//! with edge cases, large-but-bounded inputs, and property-based tests.

mod common;

use chrono::{DateTime, Duration, Utc};
use common::*;
use proptest::collection::vec as pvec;
use proptest::prelude::*;
use scotia::algebra::{
    action_graph, diff_runs, regression_suite, render_regression_suite, validate, Assertion,
    ValidationIssue,
};
use scotia::event::{ActionStatus, AgentKind, ErrorKind, ScotiaEvent, ScotiaRun};
use std::collections::HashMap;
use uuid::Uuid;

// ---------- deterministic run builder for property tests ----------

fn deterministic_run() -> ScotiaRun {
    let run_id = Uuid::from_u128(0x1234_5678_9abc_def0_0000_0000_0000_0001);
    let started_at = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    ScotiaRun {
        run_id,
        agent: AgentKind::Codex,
        task: Some("stress".to_string()),
        started_at,
        finished_at: None,
        events: vec![ScotiaEvent::RunStarted {
            run_id,
            agent: AgentKind::Codex,
            task: Some("stress".to_string()),
            timestamp: started_at,
            metadata: HashMap::new(),
        }],
        metadata: HashMap::new(),
    }
}

fn det_action_invoked(run: &mut ScotiaRun, tool: &str, target: Option<&str>, seq: usize) {
    let id = Uuid::from_u128(seq as u128);
    let timestamp = run.started_at + Duration::milliseconds(seq as i64 * 10);
    run.push(ScotiaEvent::ActionInvoked {
        event_id: id,
        run_id: run.run_id,
        timestamp,
        tool: tool.to_string(),
        target: target.map(|s| s.to_string()),
        arguments: None,
    });
}

fn det_action_result(run: &mut ScotiaRun, seq: usize) {
    let id = Uuid::from_u128(seq as u128);
    let timestamp = run.started_at + Duration::milliseconds(seq as i64 * 10);
    run.push(ScotiaEvent::ActionResult {
        event_id: id,
        run_id: run.run_id,
        timestamp,
        status: Some(ActionStatus::Success),
        stdout: None,
        stderr: None,
        exit_code: Some(0),
    });
}

fn det_finish(run: &mut ScotiaRun, seq: usize) {
    let timestamp = run.started_at + Duration::milliseconds(seq as i64 * 10);
    run.push(ScotiaEvent::RunFinished {
        run_id: run.run_id,
        timestamp,
        exit_code: Some(0),
        summary: None,
    });
    run.finished_at = Some(timestamp);
}

// ---------- proptest strategies ----------

/// A well-formed run: every `ActionInvoked` is paired with an `ActionResult`.
fn arb_balanced_run(max_pairs: usize) -> impl Strategy<Value = ScotiaRun> {
    pvec((0usize..4, 0usize..4), 0..max_pairs).prop_map(|pairs| {
        let mut run = deterministic_run();
        for (idx, (tool_idx, target_idx)) in pairs.iter().enumerate() {
            det_action_invoked(
                &mut run,
                &format!("tool{}", tool_idx),
                Some(&format!("target{}", target_idx)),
                idx * 2 + 1,
            );
            det_action_result(&mut run, idx * 2 + 2);
        }
        det_finish(&mut run, pairs.len() * 2 + 1);
        run
    })
}

/// A run where invocations may or may not be matched with results.
fn arb_unbalanced_run(max_events: usize) -> impl Strategy<Value = ScotiaRun> {
    pvec((0usize..4, 0usize..4, any::<bool>()), 0..max_events).prop_map(|specs| {
        let mut run = deterministic_run();
        for (idx, (tool_idx, target_idx, has_result)) in specs.iter().enumerate() {
            det_action_invoked(
                &mut run,
                &format!("tool{}", tool_idx),
                Some(&format!("target{}", target_idx)),
                idx * 2 + 1,
            );
            if *has_result {
                det_action_result(&mut run, idx * 2 + 2);
            }
        }
        det_finish(&mut run, specs.len() * 2 + 1);
        run
    })
}

// ---------- edge-case tests ----------

#[test]
fn validate_empty_run_reports_missing_finished() {
    let run = empty_run(AgentKind::ClaudeCode);
    let issues = validate(&run);
    assert_eq!(issues, vec![ValidationIssue::MissingRunFinished]);
}

#[test]
fn validate_duplicate_start_and_finish_events() {
    let mut run = ScotiaRun::new(AgentKind::KimiCode, Some("dup".to_string()));
    run.push(ScotiaEvent::RunStarted {
        run_id: run.run_id,
        agent: run.agent,
        task: None,
        timestamp: Utc::now(),
        metadata: HashMap::new(),
    });
    run.finish(Some(0), None);
    run.finish(Some(0), None);

    let issues = validate(&run);
    assert!(issues.contains(&ValidationIssue::MultipleRunStarted));
    assert!(issues.contains(&ValidationIssue::MultipleRunFinished));
}

#[test]
fn validate_finished_before_started() {
    let mut run = ScotiaRun::new(AgentKind::Codex, Some("time travel".to_string()));
    let run_id = run.run_id;
    let base = Utc::now();
    run.events.clear();
    run.push(ScotiaEvent::RunStarted {
        run_id,
        agent: AgentKind::Codex,
        task: None,
        timestamp: base + Duration::seconds(5),
        metadata: HashMap::new(),
    });
    run.push(ScotiaEvent::RunFinished {
        run_id,
        timestamp: base,
        exit_code: Some(0),
        summary: None,
    });

    let issues = validate(&run);
    assert!(issues.contains(&ValidationIssue::RunFinishedBeforeStarted));
}

#[test]
fn validate_nested_balanced_actions() {
    let mut run = empty_run(AgentKind::ClaudeCode);
    action_invoked(&mut run, "outer", Some("a"));
    action_invoked(&mut run, "inner", Some("b"));
    action_result(&mut run, ActionStatus::Success, 0);
    action_result(&mut run, ActionStatus::Success, 0);
    finish(&mut run, 0);

    assert!(validate(&run).is_empty());
}

#[test]
fn validate_orphaned_result_and_unmatched_invocation() {
    let mut run = empty_run(AgentKind::Agy);
    action_result(&mut run, ActionStatus::Success, 0); // orphaned
    let _id = action_invoked(&mut run, "orphan_tool", Some("x")); // unmatched
    finish(&mut run, 0);

    let issues = validate(&run);
    assert!(issues
        .iter()
        .any(|i| matches!(i, ValidationIssue::OrphanedActionResult { .. })));
    assert!(issues.iter().any(|i| matches!(
        i,
        ValidationIssue::UnmatchedActionInvoked { tool, .. }
        if tool == "orphan_tool"
    )));
}

#[test]
fn action_graph_attaches_state_deltas_to_open_action() {
    let mut run = empty_run(AgentKind::Cosine);
    action_invoked(&mut run, "edit", Some("src/lib.rs"));
    state_delta(&mut run, "src/lib.rs", "added fn");
    state_delta(&mut run, "src/lib.rs", "added test");
    action_result(&mut run, ActionStatus::Success, 0);
    finish(&mut run, 0);

    let graph = action_graph(&run);
    assert_eq!(graph.len(), 1);
    assert_eq!(graph[0].state_deltas.len(), 2);
    assert!(graph[0].result.is_some());
}

#[test]
fn regression_suite_includes_model_routes() {
    let mut run = empty_run(AgentKind::Codex);
    action_invoked(&mut run, "read", Some("src/lib.rs"));
    model_routed(&mut run, "planner", "groq");
    action_result(&mut run, ActionStatus::Success, 0);
    finish(&mut run, 0);

    let suite = regression_suite(&run);
    assert!(suite.contains(&Assertion::ModelRouted {
        stage: "planner".to_string(),
        model: "groq".to_string(),
    }));
}

#[test]
fn regression_suite_omits_no_errors_when_errors_present() {
    let mut run = empty_run(AgentKind::Codex);
    action_invoked(&mut run, "read", Some("x"));
    action_result(&mut run, ActionStatus::Success, 0);
    error_or_retry(&mut run, ErrorKind::ToolError, "boom");
    finish(&mut run, 1);

    let suite = regression_suite(&run);
    assert!(!suite.contains(&Assertion::NoErrors));
}

#[test]
fn validate_large_balanced_run_stays_valid() {
    let mut run = empty_run(AgentKind::Codex);
    for i in 0..1_000 {
        action_invoked(
            &mut run,
            &format!("tool{}", i % 4),
            Some(&format!("target{}", i % 8)),
        );
        action_result(&mut run, ActionStatus::Success, 0);
    }
    finish(&mut run, 0);

    let issues = validate(&run);
    assert!(issues.is_empty(), "large balanced run should be valid: {:?}", issues);
    assert_eq!(action_graph(&run).len(), 1_000);
}

#[test]
fn diff_runs_with_duplicate_events() {
    let mut left = empty_run(AgentKind::Codex);
    finish(&mut left, 0);

    let mut right = empty_run(AgentKind::Codex);
    action_invoked(&mut right, "bash", Some("x"));
    action_invoked(&mut right, "bash", Some("x"));
    finish(&mut right, 0);

    let diff = diff_runs(&left, &right);
    // The diff preserves multiplicity for items that are not present on the left.
    assert_eq!(diff.actions_added, vec!["bash:x", "bash:x"]);
    assert!(diff.actions_removed.is_empty());
}

// ---------- property-based tests ----------

proptest! {
    #[test]
    fn prop_validate_balanced_runs_have_no_issues(run in arb_balanced_run(100)) {
        let issues = validate(&run);
        prop_assert!(issues.is_empty(), "expected no issues, got {:?}", issues);
    }

    #[test]
    fn prop_action_graph_len_equals_invocation_count(run in arb_unbalanced_run(100)) {
        let invoked = run
            .events
            .iter()
            .filter(|e| matches!(e, ScotiaEvent::ActionInvoked { .. }))
            .count();
        let graph = action_graph(&run);
        prop_assert_eq!(graph.len(), invoked);
    }

    #[test]
    fn prop_diff_runs_self_is_empty(run in arb_balanced_run(80)) {
        let diff = diff_runs(&run, &run);
        prop_assert!(diff.actions_added.is_empty());
        prop_assert!(diff.actions_removed.is_empty());
        prop_assert!(diff.models_added.is_empty());
        prop_assert!(diff.models_removed.is_empty());
        prop_assert_eq!(diff.errors_added, 0);
        prop_assert_eq!(diff.errors_removed, 0);
    }

    #[test]
    fn prop_regression_suite_render_roundtrip(run in arb_balanced_run(60)) {
        let suite = regression_suite(&run);
        let rendered = render_regression_suite(&suite);
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&rendered)
            .expect("rendered regression suite must be valid JSON");
        prop_assert_eq!(parsed.len(), suite.len());

        for assertion in &suite {
            match assertion {
                Assertion::ToolUsed { tool, .. } => {
                    prop_assert!(
                        rendered.contains(&format!("\"tool\": \"{}\"", tool)),
                        "rendered suite missing tool {}",
                        tool
                    );
                }
                Assertion::NoErrors => {
                    prop_assert!(rendered.contains("\"kind\": \"no_errors\""));
                }
                Assertion::ActionSequence { .. } => {
                    prop_assert!(rendered.contains("\"kind\": \"action_sequence\""));
                }
                _ => {}
            }
        }
    }
}
