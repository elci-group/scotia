use scotia::event::{AgentKind, ErrorKind, ScotiaEvent};
use scotia::interceptor::{AgentInterceptor, InterceptorContext, StreamSource, build_interceptor};
use std::collections::HashMap;

fn ctx() -> InterceptorContext {
    InterceptorContext {
        run_id: uuid::Uuid::new_v4(),
        agent: AgentKind::ClaudeCode,
        hints: HashMap::new(),
    }
}

#[test]
fn name_is_claude() {
    let interceptor = build_interceptor(AgentKind::ClaudeCode);
    assert_eq!(interceptor.name(), "claude");
}

#[test]
fn parses_read_tool_invocation() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::ClaudeCode);

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "› Read: src/main.rs");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ActionInvoked { tool, target, .. } = &events[0] else {
        panic!("expected ActionInvoked, got {:?}", events[0]);
    };
    assert_eq!(tool, "read");
    assert_eq!(target.as_deref(), Some("src/main.rs"));
}

#[test]
fn parses_bash_tool_invocation() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::ClaudeCode);

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "> Bash: cargo test --lib");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ActionInvoked { tool, target, .. } = &events[0] else {
        panic!("expected ActionInvoked, got {:?}", events[0]);
    };
    assert_eq!(tool, "bash");
    assert_eq!(target.as_deref(), Some("cargo test --lib"));
}

#[test]
fn parses_alternative_tool_phrasing() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::ClaudeCode);

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "Using tool: grep");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ActionInvoked { tool, target, .. } = &events[0] else {
        panic!("expected ActionInvoked, got {:?}", events[0]);
    };
    assert_eq!(tool, "grep");
    assert_eq!(target, &None);
}

#[test]
fn accumulates_edit_diff_and_emits_state_delta() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::ClaudeCode);

    let _ = interceptor.parse_line(&ctx, StreamSource::Stdout, "› Edit: src/auth.rs");
    let _ = interceptor.parse_line(&ctx, StreamSource::Stdout, "--- a/src/auth.rs");
    let _ = interceptor.parse_line(&ctx, StreamSource::Stdout, "+++ b/src/auth.rs");
    let _ = interceptor.parse_line(
        &ctx,
        StreamSource::Stdout,
        "@@ -10,7 +10,7 @@ pub fn verify(token: &str) -> bool {",
    );
    let _ = interceptor.parse_line(&ctx, StreamSource::Stdout, "-    token.is_empty()");
    let _ = interceptor.parse_line(&ctx, StreamSource::Stdout, "+    !token.is_empty()");

    // Closing the diff block should emit the StateDelta.
    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "Done.");
    let deltas: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, ScotiaEvent::StateDelta { .. }))
        .collect();
    assert_eq!(deltas.len(), 1);
    let ScotiaEvent::StateDelta { path, diff, .. } = deltas[0] else {
        unreachable!();
    };
    assert_eq!(path.as_deref(), Some("src/auth.rs"));
    let diff = diff.as_ref().expect("diff should be present");
    assert!(diff.contains("--- a/src/auth.rs"));
    assert!(diff.contains("+    !token.is_empty()"));
}

#[test]
fn parses_model_routing_hint() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::ClaudeCode);

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "Using model: groq");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ModelRouted { stage, model, .. } = &events[0] else {
        panic!("expected ModelRouted, got {:?}", events[0]);
    };
    assert_eq!(stage, "inference");
    assert_eq!(model, "groq");
}

#[test]
fn parses_routed_to_local_model() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::ClaudeCode);

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "Routed to local");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ModelRouted { model, .. } = &events[0] else {
        panic!("expected ModelRouted, got {:?}", events[0]);
    };
    assert_eq!(model, "local");
}

#[test]
fn parses_retry_signal() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::ClaudeCode);

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "Retrying after rate limit...");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ErrorOrRetry { kind, message, .. } = &events[0] else {
        panic!("expected ErrorOrRetry, got {:?}", events[0]);
    };
    assert_eq!(*kind, ErrorKind::Retry);
    assert!(message.contains("Retrying"));
}

#[test]
fn classifies_stderr_as_error() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::ClaudeCode);

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
    let mut interceptor = build_interceptor(AgentKind::ClaudeCode);

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
    let mut interceptor = build_interceptor(AgentKind::ClaudeCode);

    let _ = interceptor.parse_line(&ctx, StreamSource::Stdout, "› Write: README.md");
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
