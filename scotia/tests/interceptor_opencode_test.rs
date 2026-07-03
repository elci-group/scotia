use scotia::event::{ActionStatus, AgentKind, ErrorKind, ScotiaEvent};
use scotia::interceptor::{AgentInterceptor, InterceptorContext, StreamSource, build_interceptor};
use scotia::interceptors::opencode::OpencodeInterceptor;
use std::collections::HashMap;
use uuid::Uuid;

fn ctx() -> InterceptorContext {
    InterceptorContext {
        run_id: Uuid::new_v4(),
        agent: AgentKind::Opencode,
        hints: HashMap::new(),
    }
}

#[test]
fn parses_bracketed_tool_invocation() {
    let ctx = ctx();
    let mut interceptor = OpencodeInterceptor::default();

    let events =
        interceptor.parse_line(&ctx, StreamSource::Stdout, "[TOOL] read_file: src/main.rs");

    assert_eq!(events.len(), 1);
    let ScotiaEvent::ActionInvoked { tool, target, .. } = &events[0] else {
        panic!("expected ActionInvoked, got {:?}", events[0]);
    };
    assert_eq!(tool, "read_file");
    assert_eq!(target.as_deref(), Some("src/main.rs"));
}

#[test]
fn parses_marker_prefixed_action_invocation() {
    let ctx = ctx();
    let mut interceptor = OpencodeInterceptor::default();

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "▸ bash: cargo test");

    assert_eq!(events.len(), 1);
    let ScotiaEvent::ActionInvoked { tool, target, .. } = &events[0] else {
        panic!("expected ActionInvoked, got {:?}", events[0]);
    };
    assert_eq!(tool, "bash");
    assert_eq!(target.as_deref(), Some("cargo test"));
}

#[test]
fn parses_model_routing_with_stage() {
    let ctx = ctx();
    let mut interceptor = OpencodeInterceptor::default();

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "[MODEL] planner: groq");

    assert_eq!(events.len(), 1);
    let ScotiaEvent::ModelRouted { stage, model, .. } = &events[0] else {
        panic!("expected ModelRouted, got {:?}", events[0]);
    };
    assert_eq!(stage, "planner");
    assert_eq!(model, "groq");
}

#[test]
fn parses_model_routing_phrase() {
    let ctx = ctx();
    let mut interceptor = OpencodeInterceptor::default();

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "Routing to ollama");

    assert_eq!(events.len(), 1);
    let ScotiaEvent::ModelRouted { stage, model, .. } = &events[0] else {
        panic!("expected ModelRouted, got {:?}", events[0]);
    };
    assert_eq!(stage, "inference");
    assert_eq!(model, "ollama");
}

#[test]
fn parses_file_edit_with_unified_diff() {
    let ctx = ctx();
    let mut interceptor = OpencodeInterceptor::default();

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "[TOOL] edit: src/auth.rs");
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ScotiaEvent::ActionInvoked { .. }));

    // Diff lines are accumulated.
    assert!(
        interceptor
            .parse_line(&ctx, StreamSource::Stdout, "--- a/src/auth.rs")
            .is_empty()
    );
    assert!(
        interceptor
            .parse_line(&ctx, StreamSource::Stdout, "+++ b/src/auth.rs")
            .is_empty()
    );
    assert!(
        interceptor
            .parse_line(&ctx, StreamSource::Stdout, "@@ -1,3 +1,3 @@")
            .is_empty()
    );
    assert!(
        interceptor
            .parse_line(&ctx, StreamSource::Stdout, "-old_line")
            .is_empty()
    );
    assert!(
        interceptor
            .parse_line(&ctx, StreamSource::Stdout, "+new_line")
            .is_empty()
    );

    // A blank/non-diff line flushes the accumulated diff.
    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "");
    assert_eq!(events.len(), 1);
    let ScotiaEvent::StateDelta { path, diff, .. } = &events[0] else {
        panic!("expected StateDelta, got {:?}", events[0]);
    };
    assert_eq!(path.as_deref(), Some("src/auth.rs"));
    let diff = diff.as_ref().expect("diff should be present");
    assert!(diff.contains("--- a/src/auth.rs"));
    assert!(diff.contains("+new_line"));
}

#[test]
fn parses_error_marker() {
    let ctx = ctx();
    let mut interceptor = OpencodeInterceptor::default();

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "[ERROR] tool failed");

    assert_eq!(events.len(), 1);
    let ScotiaEvent::ErrorOrRetry { kind, message, .. } = &events[0] else {
        panic!("expected ErrorOrRetry, got {:?}", events[0]);
    };
    assert_eq!(*kind, ErrorKind::ToolError);
    assert_eq!(message, "tool failed");
}

#[test]
fn parses_retry_marker_with_count() {
    let ctx = ctx();
    let mut interceptor = OpencodeInterceptor::default();

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "[RETRY] [2] rate limited");

    assert_eq!(events.len(), 1);
    let ScotiaEvent::ErrorOrRetry {
        kind,
        message,
        retry_count,
        ..
    } = &events[0]
    else {
        panic!("expected ErrorOrRetry, got {:?}", events[0]);
    };
    assert_eq!(*kind, ErrorKind::Retry);
    assert_eq!(message, "rate limited");
    assert_eq!(*retry_count, Some(2));
}

#[test]
fn parses_action_result() {
    let ctx = ctx();
    let mut interceptor = OpencodeInterceptor::default();

    let events = interceptor.parse_line(
        &ctx,
        StreamSource::Stdout,
        "[RESULT] status: success exit_code: 0",
    );

    assert_eq!(events.len(), 1);
    let ScotiaEvent::ActionResult {
        status, exit_code, ..
    } = &events[0]
    else {
        panic!("expected ActionResult, got {:?}", events[0]);
    };
    assert_eq!(*status, Some(ActionStatus::Success));
    assert_eq!(*exit_code, Some(0));
}

#[test]
fn parses_response_chunk() {
    let ctx = ctx();
    let mut interceptor = OpencodeInterceptor::default();

    let events = interceptor.parse_line(
        &ctx,
        StreamSource::Stdout,
        "I'll start by reading the file.",
    );

    assert_eq!(events.len(), 1);
    let ScotiaEvent::ResponseChunk { content, .. } = &events[0] else {
        panic!("expected ResponseChunk, got {:?}", events[0]);
    };
    assert_eq!(content, "I'll start by reading the file.");
}

#[test]
fn classifies_stderr_as_error() {
    let ctx = ctx();
    let mut interceptor = OpencodeInterceptor::default();

    let events = interceptor.parse_line(
        &ctx,
        StreamSource::Stderr,
        "fatal: repository does not exist",
    );

    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ScotiaEvent::ErrorOrRetry { .. }));
}

#[test]
fn parses_side_channel_json_tool() {
    let ctx = ctx();
    let mut interceptor = OpencodeInterceptor::default();

    let events = interceptor.parse_line(
        &ctx,
        StreamSource::SideChannel,
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
    assert!(arguments.is_some());
}

#[test]
fn finalize_flushes_pending_diff() {
    let ctx = ctx();
    let mut interceptor = OpencodeInterceptor::default();

    interceptor.parse_line(&ctx, StreamSource::Stdout, "[TOOL] write: src/lib.rs");
    interceptor.parse_line(&ctx, StreamSource::Stdout, "+pub fn new() {}");

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
fn build_interceptor_selects_opencode() {
    let interceptor = build_interceptor(AgentKind::Opencode);
    assert_eq!(interceptor.name(), "opencode");
}
