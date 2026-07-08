mod common;

use common::*;
use proptest::prelude::*;
use scotia::event::{ActionStatus, AgentKind, ErrorKind, ScotiaEvent};
use scotia::storage::{StorageConfig, list_runs, load_run, store_run};
use scotia::synthesizer::synthesize;
use std::collections::HashSet;
use tempfile::TempDir;

/// Build a run with `n` alternating action-invoked / action-result pairs.
fn run_with_action_pairs(agent: AgentKind, n: usize) -> scotia::event::ScotiaRun {
    let mut run = empty_run(agent);
    for i in 0..n {
        action_invoked(&mut run, "bash", Some(&format!("cmd{}", i)));
        action_result(&mut run, ActionStatus::Success, 0);
    }
    finish(&mut run, 0);
    run
}

/// Build a run whose only payload is a single large response chunk.
fn run_with_response_chunk(words: usize) -> scotia::event::ScotiaRun {
    let mut run = empty_run(AgentKind::KimiCode);
    response_chunk(&mut run, &big_text(words));
    finish(&mut run, 0);
    run
}

/// Build a run with `n` distinct action-invoked events (no results).
fn run_with_action_graph(agent: AgentKind, n: usize) -> scotia::event::ScotiaRun {
    let mut run = empty_run(agent);
    for i in 0..n {
        let tool = if i % 3 == 0 { "read" } else { "edit" };
        action_invoked(&mut run, tool, Some(&format!("src/file{}.rs", i)));
    }
    finish(&mut run, 0);
    run
}

#[tokio::test]
async fn empty_run_roundtrip() {
    let temp = TempDir::new().unwrap();
    let config = StorageConfig {
        root: temp.path().join("scotia-log"),
        commit_to_git: false,
    };
    let run = empty_run(AgentKind::ClaudeCode);
    let run_id = run.run_id;

    let stored = store_run(&config, run)
        .await
        .expect("store_run should succeed");
    assert!(stored.json_path.exists());
    assert!(stored.summary_path.exists());
    assert!(stored.dot_path.exists());

    let loaded = load_run(&stored.json_path)
        .await
        .expect("load_run should succeed");
    assert_eq!(loaded.run_id, run_id);
    assert_eq!(loaded.agent, AgentKind::ClaudeCode);
    assert!(!loaded.events.is_empty());
}

#[tokio::test]
async fn many_events_roundtrip() {
    let temp = TempDir::new().unwrap();
    let config = StorageConfig {
        root: temp.path().join("scotia-log"),
        commit_to_git: false,
    };
    let n = 10_000;
    let run = run_with_action_pairs(AgentKind::Codex, n);
    let event_count = run.events.len();
    let run_id = run.run_id;

    let stored = store_run(&config, run)
        .await
        .expect("store_run should succeed");
    let loaded = load_run(&stored.json_path)
        .await
        .expect("load_run should succeed");

    assert_eq!(loaded.run_id, run_id);
    assert_eq!(loaded.events.len(), event_count);

    // Event ordering and identity are preserved through normalization + serde.
    let actions: Vec<_> = loaded
        .events
        .iter()
        .filter_map(|e| match e {
            ScotiaEvent::ActionInvoked { tool, target, .. } => Some((tool.clone(), target.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(actions.len(), n);
    assert_eq!(actions[0].0, "bash");
    assert_eq!(actions[n - 1].1.as_deref(), Some("cmd9999"));
}

#[tokio::test]
async fn large_response_chunk_roundtrip() {
    let temp = TempDir::new().unwrap();
    let config = StorageConfig {
        root: temp.path().join("scotia-log"),
        commit_to_git: false,
    };
    let words = 10_000;
    let run = run_with_response_chunk(words);

    let stored = store_run(&config, run)
        .await
        .expect("store_run should succeed");
    let loaded = load_run(&stored.json_path)
        .await
        .expect("load_run should succeed");

    let chunks: Vec<_> = loaded
        .events
        .iter()
        .filter_map(|e| match e {
            ScotiaEvent::ResponseChunk { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect();

    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].split_whitespace().count(), words);
}

#[tokio::test]
async fn concurrent_writes_different_runs() {
    let temp = TempDir::new().unwrap();
    let config = StorageConfig {
        root: temp.path().join("scotia-log"),
        commit_to_git: false,
    };
    let configs: Vec<_> = (0..10).map(|_| config.clone()).collect();

    let mut handles = Vec::new();
    for (i, cfg) in configs.into_iter().enumerate() {
        let run = run_with_action_pairs(AgentKind::ClaudeCode, i * 100);
        handles.push(tokio::spawn(async move {
            store_run(&cfg, run)
                .await
                .expect("concurrent store should succeed")
        }));
    }

    let mut stored = Vec::new();
    for h in handles {
        stored.push(h.await.expect("task should not panic"));
    }

    let ids: HashSet<_> = stored.iter().map(|s| s.run_id).collect();
    assert_eq!(
        ids.len(),
        stored.len(),
        "each stored run must have a unique id"
    );

    for s in &stored {
        assert!(s.json_path.exists());
        let loaded = load_run(&s.json_path).await.expect("load should succeed");
        assert_eq!(loaded.run_id, s.run_id);
    }
}

#[tokio::test]
async fn list_runs_sorted() {
    let temp = TempDir::new().unwrap();
    let config = StorageConfig {
        root: temp.path().join("scotia-log"),
        commit_to_git: false,
    };

    let mut stored_ids = Vec::new();
    for i in 0..20 {
        let run = run_with_action_pairs(AgentKind::Cosine, i * 5 + 1);
        let stored = store_run(&config, run).await.expect("store should succeed");
        stored_ids.push(stored.run_id);
    }

    let paths = list_runs(&config.root)
        .await
        .expect("list_runs should succeed");
    assert_eq!(paths.len(), stored_ids.len());

    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted, "list_runs must return paths in sorted order");
}

#[test]
fn synthesize_empty_run() {
    let run = empty_run(AgentKind::Agy);
    let synthesis = synthesize(&run);

    assert!(synthesis.summary.contains(run.agent.as_str()));
    assert!(
        synthesis
            .summary
            .contains(run.task.as_deref().unwrap_or(""))
    );
    assert!(synthesis.summary.contains(&run.run_id.to_string()));
    assert!(synthesis.decision_rationales.is_empty());
    assert!(synthesis.trade_offs.is_empty());
    assert!(synthesis.action_graph_dot.starts_with("digraph scotia_run"));
    assert!(synthesis.action_graph_dot.ends_with("}\n"));
}

#[test]
fn synthesize_huge_action_graph() {
    let n = 1_000;
    let run = run_with_action_graph(AgentKind::ClaudeCode, n);
    let synthesis = synthesize(&run);

    assert!(synthesis.action_graph_dot.starts_with("digraph scotia_run"));
    assert!(synthesis.action_graph_dot.ends_with("}\n"));

    let node_lines: Vec<_> = synthesis
        .action_graph_dot
        .lines()
        .filter(|l| l.contains(" [label="))
        .collect();
    assert_eq!(node_lines.len(), n);
}

#[test]
fn synthesize_mixed_models_tradeoff() {
    let mut run = empty_run(AgentKind::Codex);
    model_routed(&mut run, "planner", "ollama");
    model_routed(&mut run, "executor", "openai/gpt-4");
    finish(&mut run, 0);

    let synthesis = synthesize(&run);
    assert!(
        synthesis
            .trade_offs
            .iter()
            .any(|t| t.contains("local and remote"))
    );
}

#[test]
fn synthesize_repeated_tool_tradeoff() {
    let mut run = empty_run(AgentKind::Opencode);
    for _ in 0..5 {
        action_invoked(&mut run, "bash", Some("cargo test"));
    }
    finish(&mut run, 0);

    let synthesis = synthesize(&run);
    assert!(
        synthesis
            .trade_offs
            .iter()
            .any(|t| t.contains("bash") && t.contains("iterative exploration"))
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn proptest_store_load_roundtrip(
        agent in prop_oneof![
            Just(AgentKind::KimiCode),
            Just(AgentKind::ClaudeCode),
            Just(AgentKind::Codex),
            Just(AgentKind::Cosine),
        ],
        pairs in 0usize..100,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let temp = TempDir::new().unwrap();
            let config = StorageConfig {
                root: temp.path().join("scotia-log"),
                commit_to_git: false,
            };
            let run = run_with_action_pairs(agent, pairs);
            let expected_events = run.events.len();
            let run_id = run.run_id;

            let stored = store_run(&config, run).await.expect("store should succeed");
            let loaded = load_run(&stored.json_path).await.expect("load should succeed");

            prop_assert_eq!(loaded.run_id, run_id);
            prop_assert_eq!(loaded.agent, agent);
            prop_assert_eq!(loaded.events.len(), expected_events);
            Ok(())
        }).unwrap();
    }

    #[test]
    fn proptest_synthesizer_invariants(
        words in 0usize..5_000,
        errors in 0usize..10,
    ) {
        let mut run = empty_run(AgentKind::KimiCode);
        response_chunk(&mut run, &big_text(words));
        for i in 0..errors {
            error_or_retry(&mut run, ErrorKind::ToolError, &format!("boom {}", i));
        }
        finish(&mut run, if errors == 0 { 0 } else { 1 });

        let synthesis = synthesize(&run);

        prop_assert!(
            synthesis.summary.contains(&run.run_id.to_string()),
            "summary should contain run id"
        );
        prop_assert!(
            synthesis.summary.contains(run.agent.as_str()),
            "summary should mention agent"
        );
        prop_assert!(
            synthesis.action_graph_dot.starts_with("digraph scotia_run"),
            "dot should start with digraph declaration"
        );
        prop_assert!(
            synthesis.action_graph_dot.ends_with("}\n"),
            "dot should end with closing brace"
        );
        prop_assert_eq!(
            synthesis.decision_rationales.len() >= errors,
            true,
            "each error/retry should contribute at least one rationale"
        );
    }
}
