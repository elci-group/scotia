use scotia::event::{AgentKind, ScotiaEvent};
use scotia::interceptor::{AgentInterceptor, InterceptorContext, StreamSource, build_interceptor};
use scotia::normalizer::normalize;
use scotia::storage::{StorageConfig, store_run};
use scotia::synthesizer::synthesize;
use scotia::wrapper::{WrapperConfig, run_and_capture};
use tempfile::TempDir;

fn ctx(agent: AgentKind) -> InterceptorContext {
    InterceptorContext {
        run_id: uuid::Uuid::new_v4(),
        agent,
        hints: std::collections::HashMap::new(),
    }
}

#[test]
fn test_claude_interceptor_parses_tool_invocations() {
    let ctx = ctx(AgentKind::ClaudeCode);
    let mut interceptor = build_interceptor(AgentKind::ClaudeCode);

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "› Read: src/main.rs");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ActionInvoked { tool, target, .. } = &events[0] else {
        panic!("expected ActionInvoked");
    };
    assert_eq!(tool, "read");
    assert_eq!(target.as_deref(), Some("src/main.rs"));
}

#[test]
fn test_codex_interceptor_parses_bracketed_tool() {
    let ctx = ctx(AgentKind::Codex);
    let mut interceptor = build_interceptor(AgentKind::Codex);

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "[bash] cargo test");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ActionInvoked { tool, target, .. } = &events[0] else {
        panic!("expected ActionInvoked");
    };
    assert_eq!(tool, "bash");
    assert_eq!(target.as_deref(), Some("cargo test"));
}

#[test]
fn test_kimi_interceptor_parses_bullet_tool() {
    let ctx = ctx(AgentKind::KimiCode);
    let mut interceptor = build_interceptor(AgentKind::KimiCode);

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "▸ file_read: src/lib.rs");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ActionInvoked { tool, target, .. } = &events[0] else {
        panic!("expected ActionInvoked");
    };
    assert_eq!(tool, "file_read");
    assert_eq!(target.as_deref(), Some("src/lib.rs"));
}

#[test]
fn test_agy_interceptor_parses_json_tool() {
    let ctx = ctx(AgentKind::Agy);
    let mut interceptor = build_interceptor(AgentKind::Agy);

    let events = interceptor.parse_line(
        &ctx,
        StreamSource::Stdout,
        r#"{"tool":"grep","target":"src","arguments":{"pattern":"fn main"}}"#,
    );
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ActionInvoked {
        tool,
        target,
        arguments,
        ..
    } = &events[0]
    else {
        panic!("expected ActionInvoked");
    };
    assert_eq!(tool, "grep");
    assert_eq!(target.as_deref(), Some("src"));
    assert!(arguments.is_some());
}

#[test]
fn test_cosine_interceptor_parses_action_line() {
    let ctx = ctx(AgentKind::Cosine);
    let mut interceptor = build_interceptor(AgentKind::Cosine);

    let events = interceptor.parse_line(
        &ctx,
        StreamSource::Stdout,
        "ACTION read_file path=src/main.rs",
    );
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ActionInvoked { tool, target, .. } = &events[0] else {
        panic!("expected ActionInvoked");
    };
    assert_eq!(tool, "read_file");
    assert_eq!(target.as_deref(), Some("src/main.rs"));
}

#[test]
fn test_opencode_interceptor_parses_tool_line() {
    let ctx = ctx(AgentKind::Opencode);
    let mut interceptor = build_interceptor(AgentKind::Opencode);

    let events =
        interceptor.parse_line(&ctx, StreamSource::Stdout, "[TOOL] read_file: src/main.rs");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ActionInvoked { tool, target, .. } = &events[0] else {
        panic!("expected ActionInvoked");
    };
    assert_eq!(tool, "read_file");
    assert_eq!(target.as_deref(), Some("src/main.rs"));
}

#[test]
fn test_normalizer_coalesces_response_chunks() {
    let mut run =
        scotia::event::ScotiaRun::new(AgentKind::ClaudeCode, Some("test".to_string()), None);
    run.push(scotia::event::ScotiaEvent::ResponseChunk {
        event_id: uuid::Uuid::new_v4(),
        run_id: run.run_id,
        timestamp: chrono::Utc::now(),
        content: "hello".to_string(),
        finish_reason: None,
    });
    run.push(scotia::event::ScotiaEvent::ResponseChunk {
        event_id: uuid::Uuid::new_v4(),
        run_id: run.run_id,
        timestamp: chrono::Utc::now(),
        content: "world".to_string(),
        finish_reason: Some("stop".to_string()),
    });

    let normalized = normalize(run);
    let chunks: Vec<_> = normalized
        .events
        .iter()
        .filter(|e| matches!(e, ScotiaEvent::ResponseChunk { .. }))
        .collect();
    assert_eq!(chunks.len(), 1);
    let ScotiaEvent::ResponseChunk {
        content,
        finish_reason,
        ..
    } = chunks[0]
    else {
        unreachable!()
    };
    assert_eq!(content, "hello\nworld");
    assert_eq!(finish_reason.as_deref(), Some("stop"));
}

#[test]
fn test_synthesizer_detects_read_then_edit_rationale() {
    use chrono::Utc;
    let mut run =
        scotia::event::ScotiaRun::new(AgentKind::ClaudeCode, Some("refactor".to_string()), None);
    run.push(scotia::event::ScotiaEvent::ActionInvoked {
        event_id: uuid::Uuid::new_v4(),
        run_id: run.run_id,
        timestamp: Utc::now(),
        tool: "read".to_string(),
        target: Some("src/auth.rs".to_string()),
        arguments: None,
    });
    run.push(scotia::event::ScotiaEvent::ActionInvoked {
        event_id: uuid::Uuid::new_v4(),
        run_id: run.run_id,
        timestamp: Utc::now(),
        tool: "edit".to_string(),
        target: Some("src/auth.rs".to_string()),
        arguments: None,
    });

    let synthesis = synthesize(&normalize(run));
    assert!(
        synthesis
            .decision_rationales
            .iter()
            .any(|r| r.contains("src/auth.rs"))
    );
}

#[tokio::test]
async fn test_wrapper_captures_echo_command() {
    let run = run_and_capture(WrapperConfig {
        agent: AgentKind::Unknown,
        task: Some("echo test".to_string()),
        program: "echo".to_string(),
        args: vec!["hello from scotia".to_string()],
        working_dir: None,
        run_id: None,
    })
    .await
    .expect("wrapper should capture echo");

    let finished = run.events.iter().find_map(|e| match e {
        ScotiaEvent::RunFinished { exit_code, .. } => Some(*exit_code),
        _ => None,
    });
    assert_eq!(finished, Some(Some(0)));
    assert!(run.events.iter().any(|e| {
        if let ScotiaEvent::ResponseChunk { content, .. } = e {
            content.contains("hello from scotia")
        } else {
            false
        }
    }));
}

#[tokio::test]
async fn test_storage_writes_run_files() {
    let temp = TempDir::new().unwrap();
    let run = scotia::event::ScotiaRun::new(AgentKind::Codex, Some("store test".to_string()), None);
    let config = StorageConfig {
        root: temp.path().join("scotia-log"),
        commit_to_git: false,
    };

    let stored = store_run(&config, run)
        .await
        .expect("store_run should succeed");
    assert!(stored.json_path.exists());
    assert!(stored.summary_path.exists());
    assert!(stored.dot_path.exists());

    let loaded = scotia::storage::load_run(&stored.json_path)
        .await
        .expect("load_run should succeed");
    assert_eq!(loaded.run_id, stored.run_id);
}
