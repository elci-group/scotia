mod common;

use proptest::prelude::*;
use scotia::event::{AgentKind, ErrorKind, ScotiaEvent};
use scotia::interceptor::{AgentInterceptor, InterceptorContext, StreamSource, build_interceptor};
use std::collections::HashMap;

fn ctx(agent: AgentKind) -> InterceptorContext {
    InterceptorContext {
        run_id: uuid::Uuid::new_v4(),
        agent,
        hints: HashMap::new(),
    }
}

fn all_agent_kinds() -> [AgentKind; 7] {
    [
        AgentKind::KimiCode,
        AgentKind::Agy,
        AgentKind::Cosine,
        AgentKind::Codex,
        AgentKind::ClaudeCode,
        AgentKind::Opencode,
        AgentKind::Unknown,
    ]
}

/// 1. Empty and whitespace-only input must never produce events for any interceptor.
#[test]
fn empty_and_whitespace_is_inert_for_all_agents() {
    for agent in all_agent_kinds() {
        let ctx = ctx(agent);
        let mut interceptor = build_interceptor(agent);
        for line in ["", "   ", "\t\n", " \n \n ", "\r\n"] {
            let events = interceptor.parse_line(&ctx, StreamSource::Stdout, line);
            assert!(
                events.is_empty(),
                "{} emitted events for whitespace input",
                agent.as_str()
            );
        }
    }
}

/// 2. Generic interceptor: classify stdout as response chunks, long stderr as errors,
///    short stderr as noise, and side-channel JSON as a response chunk.
#[test]
fn generic_garbled_and_long_lines() {
    let ctx = ctx(AgentKind::Unknown);
    let mut interceptor = build_interceptor(AgentKind::Unknown);

    let long = common::big_text(5_000);
    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, &long);
    assert_eq!(
        events.len(),
        1,
        "long stdout line should become one ResponseChunk"
    );
    assert!(
        matches!(events[0], ScotiaEvent::ResponseChunk { .. }),
        "expected ResponseChunk for stdout"
    );

    let events = interceptor.parse_line(&ctx, StreamSource::Stderr, &long);
    assert_eq!(
        events.len(),
        1,
        "long stderr line should become one ErrorOrRetry"
    );
    assert!(
        matches!(events[0], ScotiaEvent::ErrorOrRetry { .. }),
        "expected ErrorOrRetry for stderr"
    );

    let events = interceptor.parse_line(&ctx, StreamSource::Stderr, "short");
    assert!(events.is_empty(), "short stderr noise should be suppressed");

    let events = interceptor.parse_side_channel(&ctx, r#"{"tool":"x"}"#);
    assert_eq!(events.len(), 1);
    assert!(
        matches!(events[0], ScotiaEvent::ResponseChunk { .. }),
        "side-channel JSON should fall back to ResponseChunk"
    );
}

/// 3. Kimi interceptor: accumulate a large multi-line diff and flush it when a
///    non-diff line appears, plus finalization flushes any remaining buffer.
#[test]
fn kimi_large_diff_accumulates_and_flushes() {
    let ctx = ctx(AgentKind::KimiCode);
    let mut interceptor = build_interceptor(AgentKind::KimiCode);

    let start = interceptor.parse_line(&ctx, StreamSource::Stdout, "▸ edit: src/big.rs");
    assert_eq!(start.len(), 1);
    assert!(matches!(start[0], ScotiaEvent::ActionInvoked { .. }));

    const N: usize = 1_000;
    for i in 0..N {
        let line = match i % 6 {
            0 => "--- a/src/big.rs",
            1 => "+++ b/src/big.rs",
            2 => "@@ -1,3 +1,3 @@",
            3 => " old line",
            4 => "+new line",
            _ => "-removed line",
        };
        let events = interceptor.parse_line(&ctx, StreamSource::Stdout, line);
        assert!(
            events.is_empty(),
            "diff line {} should be accumulated, not emitted",
            i
        );
    }

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "Done editing.");
    assert_eq!(events.len(), 2, "expected StateDelta + ResponseChunk");

    let (path, diff) = events
        .iter()
        .find_map(|e| match e {
            ScotiaEvent::StateDelta { path, diff, .. } => Some((path.clone(), diff.clone())),
            _ => None,
        })
        .expect("StateDelta should be emitted");

    assert_eq!(path.as_deref(), Some("src/big.rs"));
    let diff = diff.expect("diff buffer should be present");
    assert_eq!(
        diff.lines().count(),
        N,
        "diff should contain every accumulated line"
    );

    let chunk = events
        .iter()
        .find_map(|e| match e {
            ScotiaEvent::ResponseChunk { content, .. } => Some(content.clone()),
            _ => None,
        })
        .expect("ResponseChunk should follow the flush");
    assert_eq!(chunk, "Done editing.");

    let final_events = interceptor.finalize(&ctx, Some(0));
    assert!(
        final_events.iter().any(|e| matches!(
            e,
            ScotiaEvent::RunFinished {
                exit_code: Some(0),
                ..
            }
        )),
        "finalize should emit RunFinished with exit code 0"
    );
}

/// 4. Agy interceptor: tolerate malformed JSON, JSON without a tool, valid JSON tools,
///    plain-text annotations, routing hints, retry signals and long garbage lines.
#[test]
fn agy_malformed_and_mixed_payloads() {
    let ctx = ctx(AgentKind::Agy);
    let mut interceptor = build_interceptor(AgentKind::Agy);

    let lines = [
        "{not json at all",
        "{\"target\": \"foo\"}",
        r#"{"tool":"read","target":"src/lib.rs","arguments":{}}"#,
        "tool: bash cargo test",
        "action: edit src/main.rs",
        "routing to local",
        "retrying operation",
        "garbled : : text :::",
        &common::big_text(1_000),
    ];

    let mut action_count = 0;
    let mut total_events = 0;
    for line in &lines {
        let events = interceptor.parse_line(&ctx, StreamSource::Stdout, line);
        total_events += events.len();
        for event in events {
            if matches!(event, ScotiaEvent::ActionInvoked { .. }) {
                action_count += 1;
            }
        }
    }

    assert_eq!(action_count, 3, "expected three ActionInvoked events");
    assert!(
        total_events >= action_count,
        "events should not be negative"
    );
}

/// 5. Codex interceptor: very long response lines are preserved, bracketed tools accept
///    large targets, and diff blocks are flushed on finalization.
#[test]
fn codex_very_long_line_and_diff_flush() {
    let ctx = ctx(AgentKind::Codex);
    let mut interceptor = build_interceptor(AgentKind::Codex);

    let long = common::big_text(5_000);
    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, &long);
    assert_eq!(events.len(), 1);
    if let ScotiaEvent::ResponseChunk { content, .. } = &events[0] {
        assert_eq!(
            content.len(),
            long.len(),
            "long response content must be preserved"
        );
    } else {
        panic!("expected ResponseChunk for long line");
    }

    let big_target = common::big_text(500);
    let line = format!("[read] {}", big_target);
    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, &line);
    assert_eq!(events.len(), 1);
    if let ScotiaEvent::ActionInvoked { target, .. } = &events[0] {
        assert_eq!(target.as_deref(), Some(big_target.as_str()));
    } else {
        panic!("expected ActionInvoked for bracketed tool");
    }

    let _ = interceptor.parse_line(&ctx, StreamSource::Stdout, "[edit] src/huge.rs");
    for i in 0..500 {
        let line = format!("+line {}", i);
        let events = interceptor.parse_line(&ctx, StreamSource::Stdout, &line);
        assert!(events.is_empty(), "diff lines should be buffered");
    }

    let final_events = interceptor.finalize(&ctx, Some(0));
    assert!(
        final_events
            .iter()
            .any(|e| matches!(e, ScotiaEvent::StateDelta { .. })),
        "finalize should flush buffered diff"
    );
    assert!(
        final_events.iter().any(|e| matches!(
            e,
            ScotiaEvent::RunFinished {
                exit_code: Some(0),
                ..
            }
        )),
        "finalize should emit RunFinished"
    );
}

/// 6. Cosine interceptor: feed a high-volume mixed stream and verify event counts.
#[test]
fn cosine_mixed_stream_burst() {
    let ctx = ctx(AgentKind::Cosine);
    let mut interceptor = build_interceptor(AgentKind::Cosine);

    const N: usize = 1_200;
    let mut routed = 0usize;
    let mut actions = 0usize;
    let mut deltas = 0usize;
    let mut chunks = 0usize;

    for i in 0..N {
        let line = match i % 6 {
            0 => "MODEL planner=groq",
            1 => "ACTION read_file path=src/main.rs",
            2 => "EDIT path=src/lib.rs applied",
            3 => "--- a/src/x.rs",
            4 => "+++ b/src/x.rs",
            _ => "plain response chunk",
        };
        let events = interceptor.parse_line(&ctx, StreamSource::Stdout, line);
        for event in events {
            match event {
                ScotiaEvent::ModelRouted { .. } => routed += 1,
                ScotiaEvent::ActionInvoked { .. } => actions += 1,
                ScotiaEvent::StateDelta { .. } => deltas += 1,
                ScotiaEvent::ResponseChunk { .. } => chunks += 1,
                _ => {}
            }
        }
    }

    assert_eq!(routed, 200, "MODEL lines");
    assert_eq!(actions, 200, "ACTION lines");
    assert_eq!(deltas, 600, "EDIT + two diff header lines per cycle");
    assert_eq!(chunks, 200, "plain response lines");
}

/// 7. Claude interceptor: routing to non-Claude models is captured, routing to Claude is
///    suppressed, diff accumulation works, and large volumes of garbage do not panic.
#[test]
fn claude_garbled_and_routing() {
    let ctx = ctx(AgentKind::ClaudeCode);
    let mut interceptor = build_interceptor(AgentKind::ClaudeCode);

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "Using model: groq");
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ScotiaEvent::ModelRouted { .. }));

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "Using model: claude");
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, ScotiaEvent::ModelRouted { .. })),
        "routing to claude itself should not be classified as ModelRouted"
    );

    let long = common::big_text(1_000);
    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, &long);
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ScotiaEvent::ResponseChunk { .. }));

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "• Edit: src/lib.rs");
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ScotiaEvent::ActionInvoked { .. }));

    for _ in 0..100 {
        let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "+some addition");
        assert!(events.is_empty(), "diff lines should be buffered");
    }

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "done");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ScotiaEvent::StateDelta { .. })),
        "non-diff line should flush buffered diff"
    );

    for _ in 0..500 {
        let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "!!!??? ::: ::");
        assert!(
            events.len() <= 1,
            "garbage should produce at most one chunk"
        );
    }
}

/// 8. Opencode interceptor: explicit markers for retry counts, errors, tools, results,
///    routing, and diff accumulation/flush.
#[test]
fn opencode_explicit_markers_and_retries() {
    let ctx = ctx(AgentKind::Opencode);
    let mut interceptor = build_interceptor(AgentKind::Opencode);

    let events = interceptor.parse_line(
        &ctx,
        StreamSource::Stdout,
        "[RETRY] [5] connection timed out",
    );
    assert_eq!(events.len(), 1);
    if let ScotiaEvent::ErrorOrRetry {
        kind, retry_count, ..
    } = &events[0]
    {
        assert_eq!(*kind, ErrorKind::Retry);
        assert_eq!(*retry_count, Some(5));
    } else {
        panic!("expected ErrorOrRetry for [RETRY]");
    }

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "[ERROR] disk full");
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        ScotiaEvent::ErrorOrRetry {
            kind: ErrorKind::ToolError,
            ..
        }
    ));

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "[TOOL] bash: ls -la");
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ScotiaEvent::ActionInvoked { .. }));

    let events = interceptor.parse_line(
        &ctx,
        StreamSource::Stdout,
        "[RESULT] status: success exit_code: 0",
    );
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ScotiaEvent::ActionResult { .. }));

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "[MODEL] planner: groq");
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ScotiaEvent::ModelRouted { .. }));

    let _ = interceptor.parse_line(&ctx, StreamSource::Stdout, "[TOOL] edit: src/x.rs");
    for i in 0..200 {
        let line = format!("+line {}", i);
        let events = interceptor.parse_line(&ctx, StreamSource::Stdout, &line);
        assert!(events.is_empty(), "diff lines should be buffered");
    }

    let events = interceptor.parse_line(&ctx, StreamSource::Stdout, "");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ScotiaEvent::StateDelta { .. })),
        "blank line should flush buffered diff"
    );
}

// 9. Property-based stress test: random printable/whitespace input never panics and
//    produces a bounded number of events, all belonging to the configured run.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn generic_interceptor_never_panics_on_random_input(
        lines in proptest::collection::vec(r"[ -~\x0a\x0d]{0,200}", 0..100)
    ) {
        let ctx = ctx(AgentKind::Unknown);
        let mut interceptor = build_interceptor(AgentKind::Unknown);
        let mut total = 0usize;

        for line in &lines {
            let stdout_events = interceptor.parse_line(&ctx, StreamSource::Stdout, line);
            let stderr_events = interceptor.parse_line(&ctx, StreamSource::Stderr, line);
            total = total.saturating_add(stdout_events.len()).saturating_add(stderr_events.len());

            for event in stdout_events.iter().chain(stderr_events.iter()) {
                prop_assert_eq!(event.run_id(), ctx.run_id);
            }
        }

        prop_assert!(total <= lines.len().saturating_mul(2));
    }
}
