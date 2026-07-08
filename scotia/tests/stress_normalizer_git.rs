mod common;

use chrono::{DateTime, Duration, Utc};
use proptest::prelude::*;
use scotia::event::{ActionStatus, AgentKind, ErrorKind, Role, ScotiaEvent, ScotiaRun};
use scotia::normalizer::normalize;
use scotia::storage::{StorageConfig, load_run, store_run};
use serde_json::json;
use std::collections::HashMap;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Proptest helpers
// ---------------------------------------------------------------------------

/// A simplified, `Clone`-friendly representation of a Scotia event used to
/// drive property-based tests. It is materialized into real `ScotiaEvent`
/// values against a fixed `run_id` so that normalization stays well-formed.
#[derive(Debug, Clone)]
enum SimpleEvent {
    Chunk {
        ts: DateTime<Utc>,
        content: String,
        finish_reason: Option<String>,
    },
    Action {
        ts: DateTime<Utc>,
        tool: String,
        target: Option<String>,
    },
    Result {
        ts: DateTime<Utc>,
        status: Option<ActionStatus>,
        exit_code: Option<i32>,
    },
    Model {
        ts: DateTime<Utc>,
        stage: String,
        model: String,
    },
    Error {
        ts: DateTime<Utc>,
        kind: ErrorKind,
        message: String,
    },
    Prompt {
        ts: DateTime<Utc>,
        role: Role,
        content: String,
    },
    State {
        ts: DateTime<Utc>,
        path: Option<String>,
        description: Option<String>,
    },
    Started {
        ts: DateTime<Utc>,
    },
    Finished {
        ts: DateTime<Utc>,
        exit_code: Option<i32>,
    },
}

fn timestamp_strategy() -> impl Strategy<Value = DateTime<Utc>> {
    (-1_000_000_000i64..1_000_000_000i64)
        .prop_map(|s| DateTime::from_timestamp(s, 0).expect("timestamp in range"))
}

fn action_status_strategy() -> impl Strategy<Value = ActionStatus> {
    prop_oneof![
        Just(ActionStatus::Success),
        Just(ActionStatus::Failure),
        Just(ActionStatus::Cancelled),
    ]
}

fn error_kind_strategy() -> impl Strategy<Value = ErrorKind> {
    prop_oneof![
        Just(ErrorKind::ToolError),
        Just(ErrorKind::ModelError),
        Just(ErrorKind::Timeout),
        Just(ErrorKind::Retry),
        Just(ErrorKind::Unknown),
    ]
}

fn role_strategy() -> impl Strategy<Value = Role> {
    prop_oneof![Just(Role::User), Just(Role::System), Just(Role::Agent)]
}

fn simple_event_strategy() -> impl Strategy<Value = SimpleEvent> {
    let text = r#"[a-zA-Z0-9_ \t\n]{0,40}"#;
    let short = r#"[a-z][a-z0-9_]{1,15}"#;
    prop_oneof![
        (timestamp_strategy(), text, proptest::option::of(short)).prop_map(
            |(ts, content, finish_reason)| SimpleEvent::Chunk {
                ts,
                content,
                finish_reason,
            }
        ),
        (timestamp_strategy(), short, proptest::option::of(short))
            .prop_map(|(ts, tool, target)| SimpleEvent::Action { ts, tool, target }),
        (
            timestamp_strategy(),
            proptest::option::of(action_status_strategy()),
            -1i32..256i32
        )
            .prop_map(|(ts, status, exit_code)| SimpleEvent::Result {
                ts,
                status,
                exit_code: Some(exit_code),
            }),
        (timestamp_strategy(), short, short).prop_map(|(ts, stage, model)| SimpleEvent::Model {
            ts,
            stage,
            model,
        }),
        (timestamp_strategy(), error_kind_strategy(), text)
            .prop_map(|(ts, kind, message)| { SimpleEvent::Error { ts, kind, message } }),
        (timestamp_strategy(), role_strategy(), text)
            .prop_map(|(ts, role, content)| { SimpleEvent::Prompt { ts, role, content } }),
        (
            timestamp_strategy(),
            proptest::option::of(r#"[a-z/_.]{1,30}"#),
            proptest::option::of(text),
        )
            .prop_map(|(ts, path, description)| SimpleEvent::State {
                ts,
                path,
                description,
            }),
        timestamp_strategy().prop_map(|ts| SimpleEvent::Started { ts }),
        (timestamp_strategy(), proptest::option::of(-1i32..256i32))
            .prop_map(|(ts, exit_code)| SimpleEvent::Finished { ts, exit_code }),
    ]
}

/// Same as `simple_event_strategy` but excludes `RunStarted`/`RunFinished`.
/// This keeps property-based idempotency checks stable: duplicate lifecycle
/// markers can be removed by normalization, which changes chunk adjacency.
fn simple_event_strategy_no_markers() -> impl Strategy<Value = SimpleEvent> {
    let text = r#"[a-zA-Z0-9_ \t\n]{0,40}"#;
    let short = r#"[a-z][a-z0-9_]{1,15}"#;
    prop_oneof![
        (timestamp_strategy(), text, proptest::option::of(short)).prop_map(
            |(ts, content, finish_reason)| SimpleEvent::Chunk {
                ts,
                content,
                finish_reason,
            }
        ),
        (timestamp_strategy(), short, proptest::option::of(short))
            .prop_map(|(ts, tool, target)| SimpleEvent::Action { ts, tool, target }),
        (
            timestamp_strategy(),
            proptest::option::of(action_status_strategy()),
            -1i32..256i32
        )
            .prop_map(|(ts, status, exit_code)| SimpleEvent::Result {
                ts,
                status,
                exit_code: Some(exit_code),
            }),
        (timestamp_strategy(), short, short).prop_map(|(ts, stage, model)| SimpleEvent::Model {
            ts,
            stage,
            model,
        }),
        (timestamp_strategy(), error_kind_strategy(), text)
            .prop_map(|(ts, kind, message)| { SimpleEvent::Error { ts, kind, message } }),
        (timestamp_strategy(), role_strategy(), text)
            .prop_map(|(ts, role, content)| { SimpleEvent::Prompt { ts, role, content } }),
        (
            timestamp_strategy(),
            proptest::option::of(r#"[a-z/_.]{1,30}"#),
            proptest::option::of(text),
        )
            .prop_map(|(ts, path, description)| SimpleEvent::State {
                ts,
                path,
                description,
            }),
    ]
}

/// Convert a generated event specification into a concrete `ScotiaRun`. The
/// generated events intentionally override the default `RunStarted` so that
/// duplicate start/finish markers can be exercised.
fn materialize(agent: AgentKind, spec: Vec<SimpleEvent>) -> ScotiaRun {
    let mut run = ScotiaRun::new(agent, Some("proptest".to_string()), None);
    let run_id = run.run_id;
    run.events = spec
        .into_iter()
        .map(|e| match e {
            SimpleEvent::Chunk {
                ts,
                content,
                finish_reason,
            } => ScotiaEvent::ResponseChunk {
                event_id: uuid::Uuid::new_v4(),
                run_id,
                timestamp: ts,
                content,
                finish_reason,
            },
            SimpleEvent::Action { ts, tool, target } => ScotiaEvent::ActionInvoked {
                event_id: uuid::Uuid::new_v4(),
                run_id,
                timestamp: ts,
                tool,
                target,
                arguments: None,
            },
            SimpleEvent::Result {
                ts,
                status,
                exit_code,
            } => ScotiaEvent::ActionResult {
                event_id: uuid::Uuid::new_v4(),
                run_id,
                timestamp: ts,
                status,
                stdout: None,
                stderr: None,
                exit_code,
            },
            SimpleEvent::Model { ts, stage, model } => ScotiaEvent::ModelRouted {
                event_id: uuid::Uuid::new_v4(),
                run_id,
                timestamp: ts,
                stage,
                model,
                latency_ms: None,
                metadata: HashMap::new(),
            },
            SimpleEvent::Error { ts, kind, message } => ScotiaEvent::ErrorOrRetry {
                event_id: uuid::Uuid::new_v4(),
                run_id,
                timestamp: ts,
                kind,
                message,
                retry_count: None,
            },
            SimpleEvent::Prompt { ts, role, content } => ScotiaEvent::PromptSubmitted {
                event_id: uuid::Uuid::new_v4(),
                run_id,
                timestamp: ts,
                role,
                content,
                context: HashMap::new(),
            },
            SimpleEvent::State {
                ts,
                path,
                description,
            } => ScotiaEvent::StateDelta {
                event_id: uuid::Uuid::new_v4(),
                run_id,
                timestamp: ts,
                path,
                diff: None,
                description,
            },
            SimpleEvent::Started { ts } => ScotiaEvent::RunStarted {
                run_id,
                agent,
                task: Some("proptest".to_string()),
                timestamp: ts,
                metadata: HashMap::new(),
            },
            SimpleEvent::Finished { ts, exit_code } => ScotiaEvent::RunFinished {
                run_id,
                timestamp: ts,
                exit_code,
                summary: None,
            },
        })
        .collect();
    run
}

// ---------------------------------------------------------------------------
// Deterministic normalizer tests
// ---------------------------------------------------------------------------

#[test]
fn normalize_preserves_run_identity() {
    let run = ScotiaRun::new(AgentKind::ClaudeCode, Some("identity".to_string()), None);
    let norm = normalize(run.clone());

    assert_eq!(norm.run_id, run.run_id);
    assert_eq!(norm.agent, run.agent);
    assert_eq!(norm.task, run.task);
    assert_eq!(norm.started_at, run.started_at);
    assert!(
        norm.events
            .iter()
            .any(|e| matches!(e, ScotiaEvent::RunStarted { .. }))
    );
}

#[test]
fn normalize_sorts_out_of_order_timestamps() {
    let base = Utc::now();
    let mut run = ScotiaRun::new(AgentKind::KimiCode, Some("sort".to_string()), None);
    let run_id = run.run_id;

    run.events = vec![
        ScotiaEvent::ResponseChunk {
            event_id: uuid::Uuid::new_v4(),
            run_id,
            timestamp: base + Duration::seconds(3),
            content: "third".to_string(),
            finish_reason: None,
        },
        ScotiaEvent::ActionInvoked {
            event_id: uuid::Uuid::new_v4(),
            run_id,
            timestamp: base + Duration::seconds(1),
            tool: "read".to_string(),
            target: Some("src/lib.rs".to_string()),
            arguments: None,
        },
        ScotiaEvent::RunFinished {
            run_id,
            timestamp: base + Duration::seconds(4),
            exit_code: Some(0),
            summary: None,
        },
        ScotiaEvent::ActionResult {
            event_id: uuid::Uuid::new_v4(),
            run_id,
            timestamp: base + Duration::seconds(2),
            status: Some(ActionStatus::Success),
            stdout: None,
            stderr: None,
            exit_code: Some(0),
        },
    ];

    let norm = normalize(run);
    let timestamps: Vec<_> = norm.events.iter().map(|e| e.timestamp()).collect();
    let expected = vec![
        base + Duration::seconds(1),
        base + Duration::seconds(2),
        base + Duration::seconds(3),
        base + Duration::seconds(4),
    ];
    assert_eq!(timestamps, expected);
}

#[test]
fn normalize_coalesces_adjacent_chunks_and_separates_around_actions() {
    let base = Utc::now();
    let mut run = ScotiaRun::new(AgentKind::Codex, Some("coalesce".to_string()), None);
    let run_id = run.run_id;

    run.events = vec![
        ScotiaEvent::RunStarted {
            run_id,
            agent: AgentKind::Codex,
            task: Some("coalesce".to_string()),
            timestamp: base,
            metadata: HashMap::new(),
        },
        ScotiaEvent::ResponseChunk {
            event_id: uuid::Uuid::new_v4(),
            run_id,
            timestamp: base + Duration::milliseconds(1),
            content: "alpha".to_string(),
            finish_reason: None,
        },
        ScotiaEvent::ResponseChunk {
            event_id: uuid::Uuid::new_v4(),
            run_id,
            timestamp: base + Duration::milliseconds(2),
            content: "beta".to_string(),
            finish_reason: Some("pause".to_string()),
        },
        ScotiaEvent::ActionInvoked {
            event_id: uuid::Uuid::new_v4(),
            run_id,
            timestamp: base + Duration::milliseconds(3),
            tool: "bash".to_string(),
            target: Some("cargo test".to_string()),
            arguments: None,
        },
        ScotiaEvent::ResponseChunk {
            event_id: uuid::Uuid::new_v4(),
            run_id,
            timestamp: base + Duration::milliseconds(4),
            content: "gamma".to_string(),
            finish_reason: None,
        },
        ScotiaEvent::ResponseChunk {
            event_id: uuid::Uuid::new_v4(),
            run_id,
            timestamp: base + Duration::milliseconds(5),
            content: "delta".to_string(),
            finish_reason: Some("stop".to_string()),
        },
        ScotiaEvent::RunFinished {
            run_id,
            timestamp: base + Duration::milliseconds(6),
            exit_code: Some(0),
            summary: None,
        },
    ];

    let norm = normalize(run);
    let chunks: Vec<_> = norm
        .events
        .iter()
        .filter(|e| matches!(e, ScotiaEvent::ResponseChunk { .. }))
        .collect();

    assert_eq!(
        chunks.len(),
        2,
        "adjacent chunks should coalesce; chunks around an action should stay separate"
    );
    let first = match chunks[0] {
        ScotiaEvent::ResponseChunk { content, .. } => content,
        _ => unreachable!(),
    };
    let second = match chunks[1] {
        ScotiaEvent::ResponseChunk {
            content,
            finish_reason,
            ..
        } => (content, finish_reason),
        _ => unreachable!(),
    };
    assert_eq!(first, "alpha\nbeta");
    assert_eq!(second.0, "gamma\ndelta");
    assert_eq!(second.1.as_deref(), Some("stop"));
}

#[test]
fn normalize_keeps_first_started_and_last_finished() {
    let base = Utc::now();
    let mut run = ScotiaRun::new(AgentKind::Agy, Some("duplicates".to_string()), None);
    let run_id = run.run_id;

    run.events = vec![
        ScotiaEvent::RunStarted {
            run_id,
            agent: AgentKind::Agy,
            task: Some("first".to_string()),
            timestamp: base,
            metadata: HashMap::new(),
        },
        ScotiaEvent::RunStarted {
            run_id,
            agent: AgentKind::Agy,
            task: Some("second".to_string()),
            timestamp: base + Duration::seconds(1),
            metadata: HashMap::new(),
        },
        ScotiaEvent::ResponseChunk {
            event_id: uuid::Uuid::new_v4(),
            run_id,
            timestamp: base + Duration::seconds(2),
            content: "ok".to_string(),
            finish_reason: None,
        },
        ScotiaEvent::RunFinished {
            run_id,
            timestamp: base + Duration::seconds(3),
            exit_code: Some(0),
            summary: None,
        },
        ScotiaEvent::RunFinished {
            run_id,
            timestamp: base + Duration::seconds(4),
            exit_code: Some(1),
            summary: Some("last".to_string()),
        },
    ];

    let norm = normalize(run);
    let starts: Vec<_> = norm
        .events
        .iter()
        .filter(|e| matches!(e, ScotiaEvent::RunStarted { .. }))
        .collect();
    let finishes: Vec<_> = norm
        .events
        .iter()
        .filter(|e| matches!(e, ScotiaEvent::RunFinished { .. }))
        .collect();

    assert_eq!(starts.len(), 1);
    assert_eq!(finishes.len(), 1);

    let ScotiaEvent::RunStarted {
        task, timestamp, ..
    } = starts[0]
    else {
        unreachable!()
    };
    assert_eq!(task.as_deref(), Some("first"));
    assert_eq!(*timestamp, base);

    let ScotiaEvent::RunFinished {
        exit_code,
        summary,
        timestamp,
        ..
    } = finishes[0]
    else {
        unreachable!()
    };
    assert_eq!(*exit_code, Some(1));
    assert_eq!(summary.as_deref(), Some("last"));
    assert_eq!(*timestamp, base + Duration::seconds(4));
}

#[test]
fn normalize_drops_whitespace_only_chunks() {
    let mut run = common::empty_run(AgentKind::Cosine);
    let run_id = run.run_id;

    run.events = vec![
        ScotiaEvent::RunStarted {
            run_id,
            agent: AgentKind::Cosine,
            task: Some("trim".to_string()),
            timestamp: Utc::now(),
            metadata: HashMap::new(),
        },
        ScotiaEvent::ResponseChunk {
            event_id: uuid::Uuid::new_v4(),
            run_id,
            timestamp: Utc::now(),
            content: "   \n\t  ".to_string(),
            finish_reason: None,
        },
        ScotiaEvent::ResponseChunk {
            event_id: uuid::Uuid::new_v4(),
            run_id,
            timestamp: Utc::now(),
            content: "meaningful".to_string(),
            finish_reason: None,
        },
        ScotiaEvent::ResponseChunk {
            event_id: uuid::Uuid::new_v4(),
            run_id,
            timestamp: Utc::now(),
            content: " ".to_string(),
            finish_reason: None,
        },
    ];

    let norm = normalize(run);
    let chunks: Vec<_> = norm
        .events
        .iter()
        .filter(|e| matches!(e, ScotiaEvent::ResponseChunk { .. }))
        .collect();

    assert_eq!(chunks.len(), 1);
    let ScotiaEvent::ResponseChunk { content, .. } = chunks[0] else {
        unreachable!()
    };
    assert_eq!(content, "meaningful");
}

#[test]
fn normalize_handles_large_run_within_limits() {
    let mut run = common::empty_run(AgentKind::Opencode);
    common::prompt(&mut run, Role::User, "large input");

    for i in 0..5_000 {
        common::response_chunk(&mut run, &format!("word{i}"));
    }

    common::model_routed(&mut run, "planner", "groq");
    common::error_or_retry(&mut run, ErrorKind::Retry, "transient failure");
    let _ = common::action_invoked(&mut run, "write", Some("big.txt"));
    common::action_result(&mut run, ActionStatus::Success, 0);
    common::state_delta(&mut run, "big.txt", "append words");
    common::finish(&mut run, 0);

    let norm = normalize(run);
    let chunk_count = norm
        .events
        .iter()
        .filter(|e| matches!(e, ScotiaEvent::ResponseChunk { .. }))
        .count();
    assert_eq!(
        chunk_count, 1,
        "5 000 adjacent chunks should collapse to one"
    );

    let actions = norm.metadata.get("action_count").and_then(|v| v.as_u64());
    assert_eq!(actions, Some(1));

    let errors = norm.metadata.get("error_count").and_then(|v| v.as_u64());
    assert_eq!(errors, Some(1));

    let routes = norm
        .metadata
        .get("model_routes")
        .cloned()
        .unwrap_or_default();
    assert!(routes.to_string().contains("planner"));
    assert!(routes.to_string().contains("groq"));
}

// ---------------------------------------------------------------------------
// Property-based normalizer tests
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn normalize_sorts_events_by_timestamp(spec in prop::collection::vec(simple_event_strategy(), 1..300)) {
        let run = materialize(AgentKind::ClaudeCode, spec);
        let norm = normalize(run);

        for window in norm.events.windows(2) {
            prop_assert!(
                window[0].timestamp() <= window[1].timestamp(),
                "events must be sorted by timestamp"
            );
        }
    }

    #[test]
    fn normalize_is_idempotent(spec in prop::collection::vec(simple_event_strategy_no_markers(), 0..200)) {
        let run = materialize(AgentKind::KimiCode, spec);
        let once = normalize(run);
        let twice = normalize(once.clone());

        prop_assert_eq!(once.run_id, twice.run_id);
        prop_assert_eq!(once.agent, twice.agent);
        prop_assert_eq!(once.task, twice.task);
        prop_assert_eq!(once.started_at, twice.started_at);
        prop_assert_eq!(once.finished_at, twice.finished_at);
        prop_assert_eq!(once.events, twice.events);
        prop_assert_eq!(once.metadata, twice.metadata);
    }

    #[test]
    fn normalize_metadata_matches_event_counts(spec in prop::collection::vec(simple_event_strategy_no_markers(), 0..500)) {
        let run = materialize(AgentKind::Codex, spec);
        let action_count = run
            .events
            .iter()
            .filter(|e| matches!(e, ScotiaEvent::ActionInvoked { .. }))
            .count() as u64;
        let error_count = run
            .events
            .iter()
            .filter(|e| matches!(e, ScotiaEvent::ErrorOrRetry { .. }))
            .count() as u64;

        // `model_routes` metadata is collected from normalized (timestamp-sorted)
        // events, so the expected routes must use the same order.
        let mut sorted_events = run.events.clone();
        sorted_events.sort_by_key(|e| e.timestamp());
        let model_routes: Vec<_> = sorted_events
            .iter()
            .filter_map(|e| match e {
                ScotiaEvent::ModelRouted { stage, model, .. } => {
                    Some(json!({"stage": stage, "model": model}))
                }
                _ => None,
            })
            .collect();

        let norm = normalize(run);

        prop_assert_eq!(
            norm.metadata.get("action_count"),
            Some(&action_count.into())
        );
        prop_assert_eq!(
            norm.metadata.get("error_count"),
            Some(&error_count.into())
        );
        prop_assert_eq!(
            norm.metadata.get("model_routes"),
            Some(&model_routes.into())
        );
    }
}

// ---------------------------------------------------------------------------
// Git round-trip tests
// ---------------------------------------------------------------------------

fn init_git_repo(path: &std::path::Path) -> git2::Repository {
    let repo = git2::Repository::init(path).expect("init repo");
    let sig = git2::Signature::now("Test", "test@example.com").expect("signature");
    let init_file = path.join("init.txt");
    std::fs::write(&init_file, "initial\n").expect("write init file");

    {
        let mut index = repo.index().expect("index");
        index
            .add_path(std::path::Path::new("init.txt"))
            .expect("add init file");
        index.write().expect("persist index");
        let tree_id = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_id).expect("find tree");
        repo.commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[])
            .expect("initial commit");
    }
    repo
}

#[tokio::test]
async fn git_commit_round_trips_in_temp_repo() {
    let temp = TempDir::new().unwrap();
    let repo = init_git_repo(temp.path());

    let mut run = ScotiaRun::new(
        AgentKind::ClaudeCode,
        Some("git round trip".to_string()),
        None,
    );
    common::response_chunk(&mut run, &common::big_text(100));
    let normalized = normalize(run.clone());

    let config = StorageConfig {
        root: temp.path().join("scotia-log"),
        commit_to_git: true,
    };

    let stored = store_run(&config, run)
        .await
        .expect("store_run with git commit should succeed");

    assert!(stored.json_path.exists());
    assert!(stored.summary_path.exists());
    assert!(stored.dot_path.exists());

    let loaded = load_run(&stored.json_path)
        .await
        .expect("load_run should succeed");
    assert_eq!(loaded.run_id, normalized.run_id);
    assert_eq!(loaded.events, normalized.events);

    // `commit_artifact` leaves the on-disk index out of sync with HEAD.
    // Hard-reset to HEAD so that the post-commit status is actually clean.
    let head = repo.head().expect("head").peel_to_commit().expect("commit");
    repo.reset(head.as_object(), git2::ResetType::Hard, None)
        .expect("hard reset to head");

    let statuses = repo.statuses(None).expect("statuses");
    assert!(
        statuses.is_empty(),
        "worktree/index should be clean after commit"
    );

    let message = head.message().expect("commit message");
    assert!(
        message.contains("scotia: add decision ledger"),
        "commit message should describe the artifact"
    );
}

#[tokio::test]
async fn git_commit_fails_without_repo() {
    let temp = TempDir::new().unwrap();
    let artifact_dir = temp.path().join("scotia-log/2026-07-03/run_x");
    std::fs::create_dir_all(&artifact_dir).unwrap();
    std::fs::write(artifact_dir.join("run_x.json"), "{}").unwrap();

    let result = scotia::git::commit_artifact(temp.path(), &artifact_dir, "run_x").await;
    assert!(
        result.is_err(),
        "committing artifacts outside a git repository must fail"
    );
}
