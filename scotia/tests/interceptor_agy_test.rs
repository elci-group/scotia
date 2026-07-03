use scotia::event::{AgentKind, ErrorKind, ScotiaEvent};
use scotia::interceptor::{AgentInterceptor, InterceptorContext, StreamSource};
use scotia::interceptors::agy::AgyInterceptor;
use std::collections::HashMap;

fn ctx(agent: AgentKind) -> InterceptorContext {
    InterceptorContext {
        run_id: uuid::Uuid::new_v4(),
        agent,
        hints: HashMap::new(),
    }
}

#[test]
fn parses_json_tool_invocation() {
    let ctx = ctx(AgentKind::Agy);
    let mut interceptor = AgyInterceptor::default();

    let events = interceptor.parse_line(
        &ctx,
        StreamSource::Stdout,
        r#"{"tool":"file_read","target":"src/main.rs","arguments":{}}"#,
    );

    assert_eq!(events.len(), 1);
    let ScotiaEvent::ActionInvoked {
        tool,
        target,
        arguments,
        ..
    } = &events[0]
    else {
        panic!("expected ActionInvoked, got {:?}", events[0]);
    };
    assert_eq!(tool, "file_read");
    assert_eq!(target.as_deref(), Some("src/main.rs"));
    assert!(arguments.is_some());
}

#[test]
fn parses_json_tool_with_arguments() {
    let ctx = ctx(AgentKind::Agy);
    let mut interceptor = AgyInterceptor::default();

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
        panic!("expected ActionInvoked, got {:?}", events[0]);
    };
    assert_eq!(tool, "grep");
    assert_eq!(target.as_deref(), Some("src"));
    assert!(arguments.as_ref().unwrap().get("pattern").is_some());
}

#[test]
fn parses_plain_text_tool_annotation() {
    let ctx = ctx(AgentKind::Agy);
    let mut interceptor = AgyInterceptor::default();

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "tool: bash cargo test");

    assert_eq!(events.len(), 1);
    let ScotiaEvent::ActionInvoked { tool, target, .. } = &events[0] else {
        panic!("expected ActionInvoked, got {:?}", events[0]);
    };
    assert_eq!(tool, "bash");
    assert_eq!(target.as_deref(), Some("cargo test"));
}

#[test]
fn parses_action_annotation() {
    let ctx = ctx(AgentKind::Agy);
    let mut interceptor = AgyInterceptor::default();

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "action: edit src/lib.rs");

    assert_eq!(events.len(), 1);
    let ScotiaEvent::ActionInvoked { tool, target, .. } = &events[0] else {
        panic!("expected ActionInvoked, got {:?}", events[0]);
    };
    assert_eq!(tool, "edit");
    assert_eq!(target.as_deref(), Some("src/lib.rs"));
}

#[test]
fn parses_model_routing_decisions() {
    let ctx = ctx(AgentKind::Agy);
    let mut interceptor = AgyInterceptor::default();

    for (line, expected) in [
        ("Routing to groq", "groq"),
        ("Using model: ollama", "ollama"),
        ("Model: openai", "openai"),
        ("Routed to local", "local"),
    ] {
        let events = interceptor.parse_line(&ctx, StreamSource::Stdout, line);
        assert_eq!(events.len(), 1, "failed for line: {line}");
        let ScotiaEvent::ModelRouted { model, .. } = &events[0] else {
            panic!("expected ModelRouted for line {line}, got {:?}", events[0]);
        };
        assert_eq!(model, expected, "for line: {line}");
    }
}

#[test]
fn parses_error_and_retry_signals() {
    let ctx = ctx(AgentKind::Agy);
    let mut interceptor = AgyInterceptor::default();

    let events = interceptor.parse_line(
        &ctx,
        StreamSource::Stdout,
        "Retrying request (attempt 2)...",
    );
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ErrorOrRetry { kind, message, .. } = &events[0] else {
        panic!("expected ErrorOrRetry, got {:?}", events[0]);
    };
    assert_eq!(*kind, ErrorKind::Retry);
    assert!(message.contains("Retrying"));

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "Error: connection refused");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ErrorOrRetry { kind, message, .. } = &events[0] else {
        panic!("expected ErrorOrRetry, got {:?}", events[0]);
    };
    assert_eq!(*kind, ErrorKind::Unknown);
    assert!(message.contains("connection refused"));
}

#[test]
fn classifies_stderr_as_error() {
    let ctx = ctx(AgentKind::Agy);
    let mut interceptor = AgyInterceptor::default();

    let events = interceptor.parse_line(&ctx, StreamSource::Stderr, "panic: something went wrong");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ErrorOrRetry { kind, message, .. } = &events[0] else {
        panic!("expected ErrorOrRetry, got {:?}", events[0]);
    };
    assert_eq!(*kind, ErrorKind::Unknown);
    assert!(message.contains("something went wrong"));
}

#[test]
fn accumulates_and_emits_diff_block() {
    let ctx = ctx(AgentKind::Agy);
    let mut interceptor = AgyInterceptor::default();

    // Start an edit.
    let events = interceptor.parse_line(
        &ctx,
        StreamSource::Stdout,
        r#"{"tool":"edit","target":"src/main.rs"}"#,
    );
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ScotiaEvent::ActionInvoked { .. }));

    // Feed diff lines.
    for line in [
        "--- src/main.rs",
        "+++ src/main.rs",
        "@@ -1,3 +1,3 @@",
        " fn old() {}",
        "+fn new() {}",
    ] {
        let events = interceptor.parse_line(&ctx, StreamSource::Stdout, line);
        assert!(events.is_empty(), "diff line should not emit yet: {line}");
    }

    // A non-diff line flushes the accumulated diff.
    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "Done editing.");
    assert_eq!(events.len(), 2, "expected StateDelta and ResponseChunk");

    let state_delta = events
        .iter()
        .find_map(|e| match e {
            ScotiaEvent::StateDelta { path, diff, .. } => Some((path.clone(), diff.clone())),
            _ => None,
        })
        .expect("expected a StateDelta");
    assert_eq!(state_delta.0.as_deref(), Some("src/main.rs"));
    assert!(
        state_delta
            .1
            .as_deref()
            .expect("diff should be present")
            .contains("fn new()")
    );

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
fn treats_unmatched_lines_as_response_chunks() {
    let ctx = ctx(AgentKind::Agy);
    let mut interceptor = AgyInterceptor::default();

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "Here is the plan:");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ResponseChunk { content, .. } = &events[0] else {
        panic!("expected ResponseChunk, got {:?}", events[0]);
    };
    assert_eq!(content, "Here is the plan:");
}
