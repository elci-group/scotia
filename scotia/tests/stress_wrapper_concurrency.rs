mod common;

use scotia::event::{AgentKind, ScotiaEvent, ScotiaRun};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn scotia_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scotia"))
}

fn run_scotia(
    program: &str,
    args: &[&str],
    stdin: Option<&[u8]>,
) -> (Output, TempDir, PathBuf) {
    let temp = TempDir::new().expect("temp dir");
    let log_root = temp.path().to_path_buf();

    let mut cmd = Command::new(scotia_bin());
    cmd.arg("--log-root")
        .arg(&log_root)
        .arg("run")
        .arg("--agent")
        .arg("unknown")
        .arg("--")
        .arg(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = if let Some(input) = stdin {
        let mut child = cmd.spawn().expect("spawn scotia");
        {
            let mut child_stdin = child.stdin.take().expect("stdin pipe");
            child_stdin
                .write_all(input)
                .expect("write to scotia stdin");
            // Dropping child_stdin closes the pipe so the wrapped process sees EOF.
        }
        child.wait_with_output().expect("wait for scotia")
    } else {
        cmd.output().expect("run scotia")
    };

    let json_path = parse_json_path(&output);
    assert!(
        json_path.exists(),
        "run json should exist: {}\nstdout:\n{}\nstderr:\n{}",
        json_path.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output, temp, json_path)
}

fn parse_json_path(output: &Output) -> PathBuf {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with("JSON:"))
        .unwrap_or_else(|| {
            panic!(
                "no JSON line in scotia output:\n{}\nstderr:\n{}",
                stdout,
                String::from_utf8_lossy(&output.stderr)
            )
        });
    let path = line.splitn(2, "JSON:").nth(1).unwrap().trim();
    PathBuf::from(path)
}

fn load_run(json_path: &PathBuf) -> ScotiaRun {
    let content = std::fs::read_to_string(json_path).expect("read run json");
    serde_json::from_str(&content).expect("parse run json")
}

fn captured_exit_code(run: &ScotiaRun) -> Option<i32> {
    run.events.iter().find_map(|e| match e {
        ScotiaEvent::RunFinished { exit_code, .. } => *exit_code,
        _ => None,
    })
}

#[test]
fn wrap_simple_echo() {
    let (output, _tmp, json_path) = run_scotia("echo", &["hello from scotia"], None);
    assert!(
        output.status.success(),
        "scotia should succeed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("hello from scotia"));

    let run = load_run(&json_path);
    assert_eq!(run.agent, AgentKind::Unknown);
    assert_eq!(captured_exit_code(&run), Some(0));
}

#[test]
fn large_stdin_echo() {
    let input = common::big_text(10_000);
    let (output, _tmp, json_path) = run_scotia("cat", &[], Some(input.as_bytes()));
    assert!(
        output.status.success(),
        "scotia should succeed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("word0"));
    assert!(stdout.contains("word9999"));

    let run = load_run(&json_path);
    assert_eq!(captured_exit_code(&run), Some(0));
    let chunks: Vec<&String> = run
        .events
        .iter()
        .filter_map(|e| match e {
            ScotiaEvent::ResponseChunk { content, .. } => Some(content),
            _ => None,
        })
        .collect();
    assert!(!chunks.is_empty(), "input should be captured as chunks");
    assert!(chunks.iter().any(|c| c.contains("word9999")));
}

#[test]
fn nonzero_exit_captured() {
    let script = "echo 'stdout-line'; echo 'stderr-line' >&2; exit 42";
    let (output, _tmp, json_path) = run_scotia("sh", &["-c", script], None);
    assert!(
        output.status.success(),
        "scotia should succeed even when wrapped process exits non-zero:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("stdout-line"));
    assert!(stderr.contains("stderr-line"));

    let run = load_run(&json_path);
    assert_eq!(captured_exit_code(&run), Some(42));
}

#[test]
fn concurrent_stdout_stderr_burst() {
    let script = "for i in {1..1000}; do echo \"out-$i\"; echo \"err-$i\" >&2; done";
    let (output, _tmp, json_path) = run_scotia("bash", &["-c", script], None);
    assert!(
        output.status.success(),
        "scotia should succeed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stdout.lines().filter(|l| l.starts_with("out-")).count(),
        1000,
        "stdout burst should be fully forwarded"
    );
    assert_eq!(
        stderr.lines().filter(|l| l.starts_with("err-")).count(),
        1000,
        "stderr burst should be fully forwarded"
    );

    let run = load_run(&json_path);
    assert_eq!(captured_exit_code(&run), Some(0));
}

#[test]
fn slow_stdout_does_not_block_fast_stderr() {
    let script = "for i in {1..500}; do echo \"err-$i\" >&2; done; sleep 0.2; echo 'done'";
    let start = Instant::now();
    let (output, _tmp, json_path) = run_scotia("bash", &["-c", script], None);
    assert!(
        output.status.success(),
        "scotia should succeed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "fast stderr should not be blocked by slow stdout"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stderr.lines().filter(|l| l.starts_with("err-")).count(),
        500
    );
    assert!(stdout.contains("done"));

    let run = load_run(&json_path);
    assert_eq!(captured_exit_code(&run), Some(0));
}

#[test]
fn empty_stdin_no_hang() {
    let start = Instant::now();
    let (output, _tmp, json_path) = run_scotia("true", &[], None);
    assert!(
        output.status.success(),
        "scotia should succeed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "wrapper should not hang on empty stdin"
    );

    let run = load_run(&json_path);
    assert_eq!(captured_exit_code(&run), Some(0));
}

#[test]
fn very_long_line() {
    let line = "x".repeat(10_000);
    let (output, _tmp, json_path) = run_scotia("printf", &["%s", &line], None);
    assert!(
        output.status.success(),
        "scotia should succeed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&line));

    let run = load_run(&json_path);
    let chunks: Vec<&String> = run
        .events
        .iter()
        .filter_map(|e| match e {
            ScotiaEvent::ResponseChunk { content, .. } => Some(content),
            _ => None,
        })
        .collect();
    assert!(chunks.iter().any(|c| c.contains(&line)));
}

#[test]
fn no_trailing_newline_stdin() {
    let payload = "payload-without-newline";
    let (output, _tmp, json_path) = run_scotia("cat", &[], Some(payload.as_bytes()));
    assert!(
        output.status.success(),
        "scotia should succeed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(payload));

    let run = load_run(&json_path);
    let chunks: Vec<&String> = run
        .events
        .iter()
        .filter_map(|e| match e {
            ScotiaEvent::ResponseChunk { content, .. } => Some(content),
            _ => None,
        })
        .collect();
    assert!(chunks.iter().any(|c| c.contains(payload)));
}

#[test]
fn garbled_mixed_sources() {
    let script =
        "for i in {1..100}; do printf 'garbage %d\n' \"$i\"; printf 'error %d\n' \"$i\" >&2; done";
    let (output, _tmp, json_path) = run_scotia("bash", &["-c", script], None);
    assert!(
        output.status.success(),
        "scotia should succeed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error 100"));

    let run = load_run(&json_path);
    assert_eq!(captured_exit_code(&run), Some(0));
}

#[test]
fn parallel_wrapper_invocations() {
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..4)
            .map(|i| {
                s.spawn(move || {
                    run_scotia("echo", &[&format!("parallel-run-{i}")], None)
                })
            })
            .collect();

        // Keep TempDirs alive alongside their JSON paths so the artifacts are still on disk
        // when we read them back.
        let mut runs = Vec::new();
        for h in handles {
            let (output, temp, json_path) = h.join().expect("thread panicked");
            assert!(
                output.status.success(),
                "every parallel scotia run should succeed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            runs.push((temp, json_path));
        }

        let ids: std::collections::HashSet<_> =
            runs.iter().map(|(_, path)| path.clone()).collect();
        assert_eq!(ids.len(), 4, "each parallel run should write a distinct artifact");

        for (i, (_, path)) in runs.iter().enumerate() {
            let run = load_run(path);
            assert_eq!(captured_exit_code(&run), Some(0));
            let contains_payload = std::fs::read_to_string(path)
                .expect("read json")
                .contains(&format!("parallel-run-{i}"));
            assert!(contains_payload, "run artifact should reference its payload");
        }
    });
}
