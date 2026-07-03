use chrono::Utc;
use scotia::algebra::{
    action_graph, diff_runs, regression_suite, render_regression_suite, validate, Assertion,
    ValidationIssue,
};
use scotia::event::{ActionStatus, AgentKind, ScotiaEvent, ScotiaRun};

#[test]
fn validate_detects_missing_run_finished() {
    let run = ScotiaRun::new(AgentKind::ClaudeCode, Some("test".to_string()));
    let issues = validate(&run);
    assert!(issues.contains(&ValidationIssue::MissingRunFinished));
}

#[test]
fn validate_detects_unmatched_action() {
    let run = ScotiaRun::new(AgentKind::Codex, Some("test".to_string()));
    let mut run = run;
    run.push(ScotiaEvent::ActionInvoked {
        event_id: uuid::Uuid::new_v4(),
        run_id: run.run_id,
        timestamp: Utc::now(),
        tool: "bash".to_string(),
        target: Some("cargo test".to_string()),
        arguments: None,
    });
    run.finish(Some(0), None);

    let issues = validate(&run);
    assert!(issues
        .iter()
        .any(|i| matches!(i, ValidationIssue::UnmatchedActionInvoked { tool, .. } if tool == "bash")));
}

#[test]
fn validate_passes_for_balanced_actions() {
    let run_id = uuid::Uuid::new_v4();
    let mut run = ScotiaRun::new(AgentKind::KimiCode, Some("test".to_string()));
    run.events = vec![run.events.remove(0)];

    let invoke_id = uuid::Uuid::new_v4();
    run.push(ScotiaEvent::ActionInvoked {
        event_id: invoke_id,
        run_id,
        timestamp: Utc::now(),
        tool: "read".to_string(),
        target: Some("src/lib.rs".to_string()),
        arguments: None,
    });
    run.push(ScotiaEvent::ActionResult {
        event_id: uuid::Uuid::new_v4(),
        run_id,
        timestamp: Utc::now(),
        status: Some(ActionStatus::Success),
        stdout: None,
        stderr: None,
        exit_code: Some(0),
    });
    run.finish(Some(0), None);

    assert!(validate(&run).is_empty());
}

#[test]
fn action_graph_pairs_invocation_and_result() {
    let run_id = uuid::Uuid::new_v4();
    let mut run = ScotiaRun::new(AgentKind::ClaudeCode, Some("test".to_string()));
    run.events = vec![run.events.remove(0)];

    run.push(ScotiaEvent::ActionInvoked {
        event_id: uuid::Uuid::new_v4(),
        run_id,
        timestamp: Utc::now(),
        tool: "edit".to_string(),
        target: Some("src/main.rs".to_string()),
        arguments: None,
    });
    run.push(ScotiaEvent::StateDelta {
        event_id: uuid::Uuid::new_v4(),
        run_id,
        timestamp: Utc::now(),
        path: Some("src/main.rs".to_string()),
        diff: None,
        description: Some("add fn".to_string()),
    });
    run.push(ScotiaEvent::ActionResult {
        event_id: uuid::Uuid::new_v4(),
        run_id,
        timestamp: Utc::now(),
        status: Some(ActionStatus::Success),
        stdout: None,
        stderr: None,
        exit_code: Some(0),
    });
    run.finish(Some(0), None);

    let graph = action_graph(&run);
    assert_eq!(graph.len(), 1);
    assert_eq!(graph[0].tool, "edit");
    assert_eq!(graph[0].state_deltas.len(), 1);
    assert!(graph[0].result.is_some());
}

#[test]
fn diff_runs_detects_added_action_and_model() {
    let run_id = uuid::Uuid::new_v4();
    let mut left = ScotiaRun::new(AgentKind::Codex, Some("left".to_string()));
    left.events = vec![left.events.remove(0)];
    left.finish(Some(0), None);

    let mut right = ScotiaRun::new(AgentKind::Codex, Some("right".to_string()));
    right.events = vec![right.events.remove(0)];
    right.push(ScotiaEvent::ActionInvoked {
        event_id: uuid::Uuid::new_v4(),
        run_id,
        timestamp: Utc::now(),
        tool: "bash".to_string(),
        target: Some("cargo test".to_string()),
        arguments: None,
    });
    right.push(ScotiaEvent::ModelRouted {
        event_id: uuid::Uuid::new_v4(),
        run_id,
        timestamp: Utc::now(),
        stage: "planner".to_string(),
        model: "groq".to_string(),
        latency_ms: None,
        metadata: Default::default(),
    });
    right.finish(Some(0), None);

    let diff = diff_runs(&left, &right);
    assert_eq!(diff.actions_added, vec!["bash:cargo test"]);
    assert_eq!(diff.models_added, vec![("planner".to_string(), "groq".to_string())]);
}

#[test]
fn regression_suite_includes_tool_model_and_sequence() {
    let run_id = uuid::Uuid::new_v4();
    let mut run = ScotiaRun::new(AgentKind::ClaudeCode, Some("regression".to_string()));
    run.events = vec![run.events.remove(0)];

    run.push(ScotiaEvent::ActionInvoked {
        event_id: uuid::Uuid::new_v4(),
        run_id,
        timestamp: Utc::now(),
        tool: "read".to_string(),
        target: Some("src/main.rs".to_string()),
        arguments: None,
    });
    run.push(ScotiaEvent::ActionInvoked {
        event_id: uuid::Uuid::new_v4(),
        run_id,
        timestamp: Utc::now(),
        tool: "edit".to_string(),
        target: Some("src/main.rs".to_string()),
        arguments: None,
    });
    run.push(ScotiaEvent::ModelRouted {
        event_id: uuid::Uuid::new_v4(),
        run_id,
        timestamp: Utc::now(),
        stage: "executor".to_string(),
        model: "ollama".to_string(),
        latency_ms: None,
        metadata: Default::default(),
    });
    run.finish(Some(0), None);

    let suite = regression_suite(&run);
    assert!(suite.contains(&Assertion::ToolUsed {
        tool: "read".to_string(),
        count: 1,
    }));
    assert!(suite.contains(&Assertion::ToolUsed {
        tool: "edit".to_string(),
        count: 1,
    }));
    assert!(suite.contains(&Assertion::ModelRouted {
        stage: "executor".to_string(),
        model: "ollama".to_string(),
    }));
    assert!(suite.contains(&Assertion::NoErrors));
    assert!(suite.iter().any(|a| matches!(a, Assertion::ActionSequence { .. })));

    let rendered = render_regression_suite(&suite);
    assert!(rendered.contains("tool_used"));
    assert!(rendered.contains("ollama"));
}
