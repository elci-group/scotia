use scotia::event::{AgentKind, ErrorKind, ScotiaEvent};
use scotia::interceptor::{AgentInterceptor, InterceptorContext, StreamSource};
use scotia::interceptors::cosine::CosineInterceptor;
use std::collections::HashMap;

fn ctx() -> InterceptorContext {
    InterceptorContext {
        run_id: uuid::Uuid::new_v4(),
        agent: AgentKind::Cosine,
        hints: HashMap::new(),
    }
}

#[test]
fn parses_read_file_action() {
    let ctx = ctx();
    let mut interp = CosineInterceptor::default();
    let events = interp.parse_line(
        &ctx,
        StreamSource::Stdout,
        "ACTION read_file path=src/main.rs",
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
    assert_eq!(tool, "read_file");
    assert_eq!(target.as_deref(), Some("src/main.rs"));
    assert!(arguments.is_some());
}

#[test]
fn parses_bash_action_without_explicit_target() {
    let ctx = ctx();
    let mut interp = CosineInterceptor::default();
    let events = interp.parse_line(&ctx, StreamSource::Stdout, "ACTION bash cargo test");
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
    assert_eq!(tool, "bash");
    assert_eq!(target.as_deref(), None);
    assert_eq!(
        arguments.as_ref().and_then(|v| v.as_str()),
        Some("cargo test")
    );
}

#[test]
fn parses_model_routing_variants() {
    let variants = [
        "MODEL planner=groq",
        "MODEL planner -> groq",
        "USING_MODEL planner groq",
        "ROUTE to planner groq",
    ];
    for line in variants {
        let ctx = ctx();
        let mut interp = CosineInterceptor::default();
        let events = interp.parse_line(&ctx, StreamSource::Stdout, line);
        assert_eq!(events.len(), 1, "failed for '{}'", line);
        let ScotiaEvent::ModelRouted { stage, model, .. } = &events[0] else {
            panic!("expected ModelRouted for '{}', got {:?}", line, events[0]);
        };
        assert_eq!(stage, "planner");
        assert_eq!(model, "groq");
    }
}

#[test]
fn parses_edit_state_delta() {
    let ctx = ctx();
    let mut interp = CosineInterceptor::default();
    let events = interp.parse_line(
        &ctx,
        StreamSource::Stdout,
        "EDIT path=src/auth.rs Added auth check",
    );
    assert_eq!(events.len(), 1);
    let ScotiaEvent::StateDelta {
        path, description, ..
    } = &events[0]
    else {
        panic!("expected StateDelta, got {:?}", events[0]);
    };
    assert_eq!(path.as_deref(), Some("src/auth.rs"));
    assert_eq!(description.as_deref(), Some("Added auth check"));
}

#[test]
fn parses_unified_diff_lines() {
    let ctx = ctx();
    let mut interp = CosineInterceptor::default();
    let events = interp.parse_line(&ctx, StreamSource::Stdout, "--- a/src/main.rs");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::StateDelta { path, diff, .. } = &events[0] else {
        panic!("expected StateDelta, got {:?}", events[0]);
    };
    assert_eq!(path.as_deref(), Some("src/main.rs"));
    assert!(diff.as_deref().unwrap().starts_with("---"));
}

#[test]
fn parses_error_and_retry_signals() {
    let ctx = ctx();
    let mut interp = CosineInterceptor::default();

    let err = interp.parse_line(&ctx, StreamSource::Stdout, "ERROR: file not found");
    assert_eq!(err.len(), 1);
    let ScotiaEvent::ErrorOrRetry { kind, message, .. } = &err[0] else {
        panic!("expected ErrorOrRetry, got {:?}", err[0]);
    };
    assert_eq!(*kind, ErrorKind::ToolError);
    assert_eq!(message, "file not found");

    let retry = interp.parse_line(&ctx, StreamSource::Stdout, "RETRY model timeout");
    assert_eq!(retry.len(), 1);
    let ScotiaEvent::ErrorOrRetry { kind, message, .. } = &retry[0] else {
        panic!("expected ErrorOrRetry, got {:?}", retry[0]);
    };
    assert_eq!(*kind, ErrorKind::Retry);
    assert_eq!(message, "model timeout");
}

#[test]
fn stderr_classified_as_error() {
    let ctx = ctx();
    let mut interp = CosineInterceptor::default();
    let events = interp.parse_line(&ctx, StreamSource::Stderr, "panic: something went wrong");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ErrorOrRetry { kind, message, .. } = &events[0] else {
        panic!("expected ErrorOrRetry, got {:?}", events[0]);
    };
    assert_eq!(*kind, ErrorKind::Unknown);
    assert_eq!(message, "panic: something went wrong");
}

#[test]
fn non_action_stdout_becomes_response_chunk() {
    let ctx = ctx();
    let mut interp = CosineInterceptor::default();
    let events = interp.parse_line(
        &ctx,
        StreamSource::Stdout,
        "Here is the summary of the file.",
    );
    assert_eq!(events.len(), 1);
    let ScotiaEvent::ResponseChunk { content, .. } = &events[0] else {
        panic!("expected ResponseChunk, got {:?}", events[0]);
    };
    assert_eq!(content, "Here is the summary of the file.");
}
