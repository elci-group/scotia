use scotia::event::{AgentKind, ErrorKind, ScotiaEvent};
use scotia::interceptor::{AgentInterceptor, InterceptorContext, StreamSource, build_interceptor};
use scotia::interceptors::kimi::KimiInterceptor;
use std::collections::HashMap;

fn ctx() -> InterceptorContext {
    InterceptorContext {
        run_id: uuid::Uuid::new_v4(),
        agent: AgentKind::KimiCode,
        hints: HashMap::new(),
    }
}

#[test]
fn name_is_kimi() {
    let interceptor = build_interceptor(AgentKind::KimiCode);
    assert_eq!(interceptor.name(), "kimi");
}

#[test]
fn parses_bash_tool_invocation_with_marker() {
    let ctx = ctx();
    let mut interceptor = KimiInterceptor::default();

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "▸ bash: cargo test");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ActionInvoked { tool, target, .. } = &events[0] else {
        panic!("expected ActionInvoked, got {:?}", events[0]);
    };
    assert_eq!(tool, "bash");
    assert_eq!(target.as_deref(), Some("cargo test"));
}

#[test]
fn parses_file_read_invocation_with_bullet() {
    let ctx = ctx();
    let mut interceptor = KimiInterceptor::default();

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "● file_read: src/main.rs");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ActionInvoked { tool, target, .. } = &events[0] else {
        panic!("expected ActionInvoked, got {:?}", events[0]);
    };
    assert_eq!(tool, "file_read");
    assert_eq!(target.as_deref(), Some("src/main.rs"));
}

#[test]
fn parses_known_tool_without_marker() {
    let ctx = ctx();
    let mut interceptor = KimiInterceptor::default();

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "grep: pattern src/");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ActionInvoked { tool, target, .. } = &events[0] else {
        panic!("expected ActionInvoked, got {:?}", events[0]);
    };
    assert_eq!(tool, "grep");
    assert_eq!(target.as_deref(), Some("pattern src/"));
}

#[test]
fn edit_starts_diff_accumulation() {
    let ctx = ctx();
    let mut interceptor = KimiInterceptor::default();

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "edit: src/lib.rs");
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ScotiaEvent::ActionInvoked { .. }));

    // Diff lines should be swallowed, not emitted as response chunks.
    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "--- src/lib.rs");
    assert!(events.is_empty());
    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "+pub fn new() {}");
    assert!(events.is_empty());
}

#[test]
fn accumulates_and_emits_diff_block() {
    let ctx = ctx();
    let mut interceptor = KimiInterceptor::default();

    let _ = interceptor.parse_line(&ctx, StreamSource::Stdout, "▸ edit: src/auth.rs");
    for line in [
        "--- a/src/auth.rs",
        "+++ b/src/auth.rs",
        "@@ -10,7 +10,7 @@",
        "     token.is_empty()",
        "+    !token.is_empty()",
    ] {
        let events = interceptor.parse_line(&ctx, StreamSource::Stdout, line);
        assert!(events.is_empty(), "diff line should not emit yet: {line}");
    }

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "Done editing.");
    assert_eq!(events.len(), 2, "expected StateDelta and ResponseChunk");

    let state_delta = events
        .iter()
        .find_map(|e| match e {
            ScotiaEvent::StateDelta { path, diff, .. } => Some((path.clone(), diff.clone())),
            _ => None,
        })
        .expect("expected a StateDelta");
    assert_eq!(state_delta.0.as_deref(), Some("src/auth.rs"));
    let diff = state_delta.1.as_deref().expect("diff should be present");
    assert!(diff.contains("--- a/src/auth.rs"));
    assert!(diff.contains("+    !token.is_empty()"));

    let response_chunk = events
        .iter()
        .find_map(|e| match e {
            ScotiaEvent::ResponseChunk { content, .. } => Some(content.clone()),
            _ => None,
        })
        .expect("expected a ResponseChunk");
    assert_eq!(response_chunk, "Done editing.");
}

#[test]
fn parses_model_routing_decisions() {
    let ctx = ctx();
    let mut interceptor = KimiInterceptor::default();

    for (line, expected_stage, expected_model) in [
        ("Thinking with groq", "thinking", "groq"),
        ("executor: ollama", "executor", "ollama"),
        ("model: openai", "model", "openai"),
        ("Routing to local", "routing", "local"),
    ] {
        let events = interceptor.parse_line(&ctx, StreamSource::Stdout, line);
        assert_eq!(events.len(), 1, "failed for line: {line}");
        let ScotiaEvent::ModelRouted { stage, model, .. } = &events[0] else {
            panic!("expected ModelRouted for line {line}, got {:?}", events[0]);
        };
        assert_eq!(stage, expected_stage, "stage mismatch for line: {line}");
        assert_eq!(model, expected_model, "model mismatch for line: {line}");
    }
}

#[test]
fn parses_retry_signal() {
    let ctx = ctx();
    let mut interceptor = KimiInterceptor::default();

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "Retrying after timeout...");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ErrorOrRetry { kind, message, .. } = &events[0] else {
        panic!("expected ErrorOrRetry, got {:?}", events[0]);
    };
    assert_eq!(*kind, ErrorKind::Retry);
    assert!(message.contains("Retrying"));
}

#[test]
fn parses_timeout_signal() {
    let ctx = ctx();
    let mut interceptor = KimiInterceptor::default();

    let events =
        interceptor.parse_line(&ctx, StreamSource::Stdout, "timed out waiting for response");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ErrorOrRetry { kind, .. } = &events[0] else {
        panic!("expected ErrorOrRetry, got {:?}", events[0]);
    };
    assert_eq!(*kind, ErrorKind::Timeout);
}

#[test]
fn parses_model_error_signal() {
    let ctx = ctx();
    let mut interceptor = KimiInterceptor::default();

    let events = interceptor.parse_line(
        &ctx,
        StreamSource::Stdout,
        "Error: model returned invalid JSON",
    );
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ErrorOrRetry { kind, .. } = &events[0] else {
        panic!("expected ErrorOrRetry, got {:?}", events[0]);
    };
    assert_eq!(*kind, ErrorKind::ModelError);
}

#[test]
fn classifies_stderr_as_error() {
    let ctx = ctx();
    let mut interceptor = KimiInterceptor::default();

    let events = interceptor.parse_line(
        &ctx,
        StreamSource::Stderr,
        "thread 'main' panicked at src/lib.rs:42",
    );
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ErrorOrRetry { kind, .. } = &events[0] else {
        panic!("expected ErrorOrRetry, got {:?}", events[0]);
    };
    assert_eq!(*kind, ErrorKind::Unknown);
}

#[test]
fn treats_plain_text_as_response_chunk() {
    let ctx = ctx();
    let mut interceptor = KimiInterceptor::default();

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "Let me think about this...");
    assert_eq!(events.len(), 1);
    assert!(
        matches!(events[0], ScotiaEvent::ResponseChunk { .. }),
        "expected ResponseChunk, got {:?}",
        events[0]
    );
}

#[test]
fn finalize_flushes_open_diff_and_run_finished() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::KimiCode);

    let _ = interceptor.parse_line(&ctx, StreamSource::Stdout, "▸ write: README.md");
    let _ = interceptor.parse_line(&ctx, StreamSource::Stdout, "+ # Scotia");
    let _ = interceptor.parse_line(
        &ctx,
        StreamSource::Stdout,
        "+ A Semantic Decision Ledger for agentic systems.",
    );

    let events = interceptor.finalize(&ctx, Some(0));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ScotiaEvent::StateDelta { .. }))
    );
    assert!(events.iter().any(|e| matches!(
        e,
        ScotiaEvent::RunFinished {
            exit_code: Some(0),
            ..
        }
    )));
}
