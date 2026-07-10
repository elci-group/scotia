//! Criterion micro-benchmarks for Scotia's hot, pure paths.
//!
//! These cover the functions most likely to be affected by the god-object
//! factorisation (algebra, interceptors, event). The goal is not peak speed but
//! proving the refactor is perf-neutral: capture a baseline on the pre-refactor
//! code (`cargo bench -- --save-baseline before`) and compare after
//! (`cargo bench -- --baseline before`).

use chrono::Utc;
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use scotia::algebra;
use scotia::event::{ActionStatus, AgentKind, ErrorKind, ScotiaEvent, ScotiaRun};
use scotia::interceptor::{AgentInterceptor, InterceptorContext, StreamSource, build_interceptor};
use uuid::Uuid;

/// Build a deterministic, balanced run with a mix of every event kind so the
/// algebra benches exercise all arms.
fn fixture(pairs: usize) -> ScotiaRun {
    let run_id = Uuid::new_v4();
    let mut run = ScotiaRun::new(AgentKind::Codex, Some("bench".into()), Some(run_id));
    let base = Utc::now();
    for i in 0..pairs {
        let ts = base + chrono::Duration::milliseconds(i as i64);
        run.push(ScotiaEvent::ActionInvoked {
            event_id: Uuid::new_v4(),
            run_id,
            timestamp: ts,
            tool: if i % 3 == 0 { "shell" } else { "edit" }.into(),
            target: Some(format!("file_{i}.rs")),
            arguments: None,
        });
        run.push(ScotiaEvent::ActionResult {
            event_id: Uuid::new_v4(),
            run_id,
            timestamp: ts,
            status: Some(ActionStatus::Success),
            stdout: Some("ok".into()),
            stderr: None,
            exit_code: Some(0),
        });
        if i % 5 == 0 {
            run.push(ScotiaEvent::ModelRouted {
                event_id: Uuid::new_v4(),
                run_id,
                timestamp: ts,
                stage: "executor".into(),
                model: "local".into(),
                latency_ms: Some(12),
                metadata: Default::default(),
            });
        }
        if i % 7 == 0 {
            run.push(ScotiaEvent::StateDelta {
                event_id: Uuid::new_v4(),
                run_id,
                timestamp: ts,
                path: Some(format!("src/file_{i}.rs")),
                diff: Some("+line".into()),
                description: Some("edit".into()),
            });
        }
    }
    run.push(ScotiaEvent::ErrorOrRetry {
        event_id: Uuid::new_v4(),
        run_id,
        timestamp: base,
        kind: ErrorKind::Retry,
        message: "transient".into(),
        retry_count: Some(1),
    });
    run.finish(Some(0), Some("done".into()));
    run
}

fn bench_algebra(c: &mut Criterion) {
    let run = fixture(200);
    let other = fixture(200);
    let mut g = c.benchmark_group("algebra");
    g.bench_function("validate", |b| {
        b.iter(|| algebra::validate(black_box(&run)))
    });
    g.bench_function("action_graph", |b| {
        b.iter(|| algebra::action_graph(black_box(&run)))
    });
    g.bench_function("diff_runs", |b| {
        b.iter(|| algebra::diff_runs(black_box(&run), black_box(&other)))
    });
    g.bench_function("regression_suite", |b| {
        b.iter(|| algebra::regression_suite(black_box(&run)))
    });
    let suite = algebra::regression_suite(&run);
    g.bench_function("render_regression_suite", |b| {
        b.iter(|| algebra::render_regression_suite(black_box(&suite)))
    });
    g.finish();
}

fn bench_interceptors(c: &mut Criterion) {
    let lines: &[&str] = &[
        "[shell] src/main.rs",
        r#"{"tool":"edit","target":"src/lib.rs","arguments":{}}"#,
        "▸ read: Cargo.toml",
        "model routed: planner -> groq",
        "error: tool failed, retrying",
        "```diff\n@@\n-old\n+new\n```",
        "some free-form response text",
    ];
    let mut g = c.benchmark_group("interceptors");
    for kind in [
        AgentKind::Codex,
        AgentKind::Agy,
        AgentKind::ClaudeCode,
        AgentKind::KimiCode,
        AgentKind::Opencode,
    ] {
        let ctx = InterceptorContext {
            run_id: scotia::interceptor::new_event_id(),
            agent: kind,
            hints: Default::default(),
        };
        g.bench_function(format!("parse_line/{}", kind.as_str()), |b| {
            b.iter(|| {
                let mut it = build_interceptor(kind);
                for line in lines {
                    let _ = it.parse_line(black_box(&ctx), StreamSource::Stdout, black_box(line));
                }
            })
        });
    }
    g.finish();
}

fn bench_event(c: &mut Criterion) {
    let run = fixture(20);
    let first = run.events[0].clone();
    let mut g = c.benchmark_group("event");
    g.bench_function("from_binary_name", |b| {
        b.iter(|| AgentKind::from_binary_name(black_box("claude-code")))
    });
    g.bench_function("as_str", |b| b.iter(|| AgentKind::ClaudeCode.as_str()));
    g.bench_function("timestamp", |b| b.iter(|| black_box(&first).timestamp()));
    g.bench_function("run_id", |b| b.iter(|| black_box(&first).run_id()));
    g.bench_function("scotia_run_new_push_finish", |b| {
        b.iter(|| {
            let mut r = ScotiaRun::new(AgentKind::Agy, None, None);
            r.push(first.clone());
            r.finish(Some(0), None);
            black_box(r)
        })
    });
    g.finish();
}

criterion_group!(benches, bench_algebra, bench_interceptors, bench_event);
criterion_main!(benches);
