use scotia::event::{AgentKind, ErrorKind, ScotiaEvent};
use scotia::interceptor::{AgentInterceptor, InterceptorContext, StreamSource, build_interceptor};
use std::collections::HashMap;

fn ctx() -> InterceptorContext {
    InterceptorContext {
        run_id: uuid::Uuid::new_v4(),
        agent: AgentKind::Codex,
        hints: HashMap::new(),
    }
}

#[test]
fn build_interceptor_returns_codex() {
    let interceptor = build_interceptor(AgentKind::Codex);
    assert_eq!(interceptor.name(), "codex");
}

#[test]
fn parses_bracketed_bash_invocation() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::Codex);

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "[bash] cargo test");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ActionInvoked { tool, target, .. } = &events[0] else {
        panic!("expected ActionInvoked, got {:?}", events[0]);
    };
    assert_eq!(tool, "bash");
    assert_eq!(target.as_deref(), Some("cargo test"));
}

#[test]
fn parses_bracketed_file_read() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::Codex);

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "[read] src/main.rs");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ActionInvoked { tool, target, .. } = &events[0] else {
        panic!("expected ActionInvoked");
    };
    assert_eq!(tool, "read");
    assert_eq!(target.as_deref(), Some("src/main.rs"));
}

#[test]
fn parses_json_tool_call_with_arguments() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::Codex);

    let line = r#"{"tool":"grep","target":"src","arguments":{"pattern":"fn main"}}"#;
    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, line);
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
fn parses_colon_style_tool_invocation() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::Codex);

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "▸ edit: src/lib.rs");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ActionInvoked { tool, target, .. } = &events[0] else {
        panic!("expected ActionInvoked");
    };
    assert_eq!(tool, "edit");
    assert_eq!(target.as_deref(), Some("src/lib.rs"));
}

#[test]
fn parses_model_routing_with_colon() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::Codex);

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "model: groq");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ModelRouted { stage, model, .. } = &events[0] else {
        panic!("expected ModelRouted");
    };
    assert_eq!(stage, "generation");
    assert_eq!(model, "groq");
}

#[test]
fn parses_model_routing_using() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::Codex);

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "Using ollama for inference");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ModelRouted { model, .. } = &events[0] else {
        panic!("expected ModelRouted");
    };
    assert_eq!(model, "ollama");
}

#[test]
fn parses_routed_to_openai() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::Codex);

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "Routed to openai");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ModelRouted { model, .. } = &events[0] else {
        panic!("expected ModelRouted");
    };
    assert_eq!(model, "openai");
}

#[test]
fn parses_retry_signal() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::Codex);

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "Retrying after rate limit...");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ErrorOrRetry { kind, message, .. } = &events[0] else {
        panic!("expected ErrorOrRetry");
    };
    assert_eq!(*kind, ErrorKind::Retry);
    assert!(message.contains("Retrying"));
}

#[test]
fn parses_error_signal() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::Codex);

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "Error: command failed");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ErrorOrRetry { kind, .. } = &events[0] else {
        panic!("expected ErrorOrRetry");
    };
    assert_eq!(*kind, ErrorKind::Unknown);
}

#[test]
fn parses_stderr_as_error() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::Codex);

    let events = interceptor.parse_line(&ctx, StreamSource::Stderr, "fatal: unable to connect");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ErrorOrRetry { kind, message, .. } = &events[0] else {
        panic!("expected ErrorOrRetry");
    };
    assert_eq!(*kind, ErrorKind::Unknown);
    assert!(message.contains("unable to connect"));
}

#[test]
fn accumulates_and_emits_diff_block() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::Codex);

    interceptor.parse_line(&ctx, StreamSource::Stdout, "[edit] src/main.rs");
    interceptor.parse_line(&ctx, StreamSource::Stdout, "--- a/src/main.rs");
    interceptor.parse_line(&ctx, StreamSource::Stdout, "+++ b/src/main.rs");
    interceptor.parse_line(&ctx, StreamSource::Stdout, "@@ -1,3 +1,4 @@");
    interceptor.parse_line(&ctx, StreamSource::Stdout, "+fn new_feature() {}");
    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "Done editing.");

    let delta = events.iter().find_map(|e| match e {
        ScotiaEvent::StateDelta { path, diff, .. } => Some((path.clone(), diff.clone())),
        _ => None,
    });
    assert!(delta.is_some(), "expected a StateDelta event");
    let (path, diff) = delta.unwrap();
    assert_eq!(path.as_deref(), Some("src/main.rs"));
    assert!(diff.as_ref().unwrap().contains("+fn new_feature()"));
}

#[test]
fn finalize_flushes_pending_diff() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::Codex);

    interceptor.parse_line(&ctx, StreamSource::Stdout, "[write] README.md");
    interceptor.parse_line(&ctx, StreamSource::Stdout, "+ # Scotia");
    let events = interceptor.finalize(&ctx, Some(0));

    assert!(
        events
            .iter()
            .any(|e| matches!(e, ScotiaEvent::StateDelta { .. }))
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ScotiaEvent::RunFinished { .. }))
    );
}

#[test]
fn parses_plain_response_chunk() {
    let ctx = ctx();
    let mut interceptor = build_interceptor(AgentKind::Codex);

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "I'll help with that.");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ResponseChunk { content, .. } = &events[0] else {
        panic!("expected ResponseChunk");
    };
    assert_eq!(content, "I'll help with that.");
}
