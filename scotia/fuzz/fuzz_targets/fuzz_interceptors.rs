#![no_main]

//! Fuzz every agent interceptor's line parser.
//!
//! Interceptors turn raw, attacker-influencerable agent telemetry (stdout,
//! stderr, side-channel JSON) into canonical Scotia events. A panic here would
//! crash the wrapper around the agent, so each parser must tolerate arbitrary
//! input. The first byte selects the interceptor; the rest is fed as lossy
//! UTF-8, line by line (mimicking the wrapper's framing), then once through the
//! side-channel path and finalizer.

use libfuzzer_sys::fuzz_target;
use scotia::event::AgentKind;
use scotia::interceptor::{build_interceptor, InterceptorContext, StreamSource};

const KINDS: &[AgentKind] = &[
    AgentKind::ClaudeCode,
    AgentKind::Codex,
    AgentKind::KimiCode,
    AgentKind::Agy,
    AgentKind::Cosine,
    AgentKind::Opencode,
];

fuzz_target!(|data: &[u8]| {
    if data.is_empty() || data.len() > 64 * 1024 {
        return;
    }
    let (head, rest) = data.split_at(1);
    let kind = KINDS[(head[0] as usize) % KINDS.len()];

    let mut interceptor = build_interceptor(kind);
    let ctx = InterceptorContext {
        run_id: scotia::interceptor::new_event_id(),
        agent: kind,
        hints: Default::default(),
    };

    let text = String::from_utf8_lossy(rest);
    for line in text.lines() {
        let _ = interceptor.parse_line(&ctx, StreamSource::Stdout, line);
    }
    let _ = interceptor.parse_side_channel(&ctx, &text);
    let _ = interceptor.finalize(&ctx, Some(0));
});
