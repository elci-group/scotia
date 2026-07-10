use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use chrono::Utc;
use proptest::prelude::*;
use serde_json::Value;
use tempfile::TempDir;
use uuid::Uuid;

mod common;
use common::{
    action_invoked, action_result, big_text, empty_run, finish, model_routed, state_delta,
};

/// Return the path to the `scotia` binary compiled for this test run.
fn scotia_bin() -> PathBuf {
    PathBuf::from(option_env!("CARGO_BIN_EXE_scotia").unwrap_or("target/debug/scotia"))
}

/// Spawn the Scotia CLI with the given global + subcommand arguments.
fn scotia<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    Command::new(scotia_bin())
        .args(args)
        .output()
        .expect("failed to execute scotia binary")
}

/// Find the most recently written `.json` run file under a log root.
fn latest_json(root: &Path) -> PathBuf {
    let mut latest: Option<(PathBuf, std::time::SystemTime)> = None;
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_dir() {
            continue;
        }
        for sub in fs::read_dir(entry.path()).unwrap() {
            let sub = sub.unwrap();
            let path = sub.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let modified = fs::metadata(&path).unwrap().modified().unwrap();
                if latest.as_ref().map(|(_, t)| modified > *t).unwrap_or(true) {
                    latest = Some((path, modified));
                }
            }
        }
    }
    latest
        .map(|(p, _)| p)
        .expect("no run JSON found under log root")
}

/// Create a temporary log root and return the TempDir guard and its path.
fn temp_log_root() -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("scotia-log");
    fs::create_dir_all(&root).unwrap();
    (dir, root)
}

/// Run a captured command through the CLI and return the generated JSON path.
fn cli_run(root: &Path, agent: &str, task: &str, program: &str, args: &[&str]) -> PathBuf {
    let mut cli_args: Vec<String> = vec![
        "--log-root".into(),
        root.to_string_lossy().into_owned(),
        "run".into(),
        "--agent".into(),
        agent.into(),
        "--task".into(),
        task.into(),
        "--".into(),
        program.into(),
    ];
    for a in args {
        cli_args.push((*a).into());
    }

    let output = scotia(&cli_args);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "scotia run failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        stderr
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Scotia captured run"),
        "unexpected run output: {}",
        stdout
    );

    latest_json(root)
}

#[test]
fn run_captures_echo_and_creates_artifacts() {
    let (_dir, root) = temp_log_root();
    let json = cli_run(
        &root,
        "generic",
        "echo smoke",
        "/bin/sh",
        &["-c", "echo 'hello scotia'"],
    );

    assert!(json.exists());
    let summary = json.with_extension("summary.md");
    let dot = json.with_extension("dot");
    assert!(summary.exists());
    assert!(dot.exists());

    let validate_out = scotia([
        "--log-root",
        &root.to_string_lossy(),
        "validate",
        &json.to_string_lossy(),
    ]);
    let validate = String::from_utf8_lossy(&validate_out.stdout);
    assert!(
        validate.contains("structurally valid"),
        "validate output: {}",
        validate
    );
}

#[test]
fn run_stderr_classifies_long_errors() {
    let (_dir, root) = temp_log_root();
    let json = cli_run(
        &root,
        "generic",
        "stderr smoke",
        "/bin/sh",
        &[
            "-c",
            "echo 'stdout line'; echo 'this is a simulated long stderr error message' >&2",
        ],
    );

    let run_value: Value = serde_json::from_str(&fs::read_to_string(&json).unwrap()).unwrap();
    let errors = run_value["metadata"]["error_count"].as_u64().unwrap_or(0);
    assert!(
        errors >= 1,
        "expected at least one classified error, got {}",
        errors
    );

    let reg_out = scotia([
        "--log-root",
        &root.to_string_lossy(),
        "regression",
        &json.to_string_lossy(),
    ]);
    let regression = String::from_utf8_lossy(&reg_out.stdout);
    assert!(
        !regression.contains("no_errors"),
        "regression should not claim no_errors when stderr was classified: {}",
        regression
    );
}

#[test]
fn list_reports_runs() {
    let (_dir, root) = temp_log_root();

    let empty = scotia(["--log-root", &root.to_string_lossy(), "list"]);
    let empty_stdout = String::from_utf8_lossy(&empty.stdout);
    assert!(empty_stdout.contains("No Scotia runs found"));

    cli_run(
        &root,
        "generic",
        "list smoke",
        "/bin/sh",
        &["-c", "echo listed"],
    );

    let populated = scotia(["--log-root", &root.to_string_lossy(), "list"]);
    let populated_stdout = String::from_utf8_lossy(&populated.stdout);
    assert!(!populated_stdout.contains("No Scotia runs found"));
    assert!(populated_stdout.contains(".json"));
}

#[test]
fn validate_detects_orphan_action_result() {
    let (_dir, root) = temp_log_root();
    let json = cli_run(
        &root,
        "generic",
        "validate orphan",
        "/bin/sh",
        &["-c", "echo ok"],
    );

    let mut run_value: Value = serde_json::from_str(&fs::read_to_string(&json).unwrap()).unwrap();
    let run_id = run_value["run_id"].as_str().unwrap().to_string();
    let events = run_value["events"].as_array_mut().unwrap();
    // Insert an orphaned ActionResult before any ActionInvoked.
    events.insert(
        1,
        serde_json::json!({
            "type": "action_result",
            "event_id": Uuid::new_v4().to_string(),
            "run_id": run_id,
            "timestamp": Utc::now().to_rfc3339(),
            "status": "failure",
            "exit_code": 1,
        }),
    );
    fs::write(&json, serde_json::to_string_pretty(&run_value).unwrap()).unwrap();

    let out = scotia([
        "--log-root",
        &root.to_string_lossy(),
        "validate",
        &json.to_string_lossy(),
    ]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("OrphanedActionResult") || stdout.contains("validation issue"),
        "expected validation to flag orphan action result: {}",
        stdout
    );
}

#[test]
fn diff_reports_action_and_model_differences() {
    let (_dir, root) = temp_log_root();

    let left = {
        let mut run = empty_run(scotia::event::AgentKind::ClaudeCode);
        action_invoked(&mut run, "read", Some("src/a.rs"));
        model_routed(&mut run, "planner", "gpt-4");
        finish(&mut run, 0);
        persist_run(&root, run)
    };

    let right = {
        let mut run = empty_run(scotia::event::AgentKind::Codex);
        action_invoked(&mut run, "write", Some("src/b.rs"));
        model_routed(&mut run, "executor", "claude-3");
        finish(&mut run, 0);
        persist_run(&root, run)
    };

    let out = scotia(["--log-root", &root.to_string_lossy(), "diff", &left, &right]);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        stdout.contains("read:src/a.rs"),
        "left-only action missing: {}",
        stdout
    );
    assert!(
        stdout.contains("write:src/b.rs"),
        "right-only action missing: {}",
        stdout
    );
    assert!(
        stdout.contains("planner"),
        "left-only model missing: {}",
        stdout
    );
    assert!(
        stdout.contains("gpt-4"),
        "left-only model missing: {}",
        stdout
    );
    assert!(
        stdout.contains("executor"),
        "right-only model missing: {}",
        stdout
    );
    assert!(
        stdout.contains("claude-3"),
        "right-only model missing: {}",
        stdout
    );
}

#[test]
fn regression_emits_expected_assertions() {
    let (_dir, root) = temp_log_root();
    let json = {
        let mut run = empty_run(scotia::event::AgentKind::KimiCode);
        let _id = action_invoked(&mut run, "grep", Some("src"));
        action_result(&mut run, scotia::event::ActionStatus::Success, 0);
        model_routed(&mut run, "planner", "gpt-4o");
        state_delta(&mut run, "src/main.rs", "added auth check");
        finish(&mut run, 0);
        persist_run(&root, run)
    };

    let out = scotia(["--log-root", &root.to_string_lossy(), "regression", &json]);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        stdout.contains("tool_used"),
        "missing tool_used: {}",
        stdout
    );
    assert!(
        stdout.contains("model_routed"),
        "missing model_routed: {}",
        stdout
    );
    assert!(
        stdout.contains("state_changed"),
        "missing state_changed: {}",
        stdout
    );
    assert!(
        stdout.contains("no_errors"),
        "missing no_errors: {}",
        stdout
    );
    assert!(
        stdout.contains("action_sequence"),
        "missing action_sequence: {}",
        stdout
    );
}

#[test]
fn summary_command_renders_markdown() {
    let (_dir, root) = temp_log_root();
    let json = cli_run(
        &root,
        "generic",
        "summary smoke",
        "/bin/sh",
        &["-c", "echo 'summary line'"],
    );

    let out = scotia([
        "--log-root",
        &root.to_string_lossy(),
        "summary",
        &json.to_string_lossy(),
    ]);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        stdout.contains("# Scotia Run Summary"),
        "summary header missing: {}",
        stdout
    );
    assert!(
        stdout.contains("summary smoke"),
        "task missing from summary: {}",
        stdout
    );
}

#[test]
fn large_input_run_with_10k_words() {
    let (dir, root) = temp_log_root();
    let text = big_text(10_000);
    let input_path = dir.path().join("big_input.txt");
    fs::write(&input_path, &text).unwrap();

    let json = cli_run(
        &root,
        "generic",
        "large input",
        "/bin/sh",
        &["-c", &format!("cat {}", input_path.display())],
    );

    assert!(json.exists());
    let run_value: Value = serde_json::from_str(&fs::read_to_string(&json).unwrap()).unwrap();
    let chunks: Vec<&Value> = run_value["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "response_chunk")
        .collect();
    assert!(!chunks.is_empty(), "expected at least one response chunk");
    let content = chunks[0]["content"].as_str().unwrap_or("");
    assert!(content.contains("word0"), "start of large input missing");
    assert!(content.contains("word9999"), "end of large input missing");
}

#[test]
fn missing_agent_binary_fails_cleanly() {
    let (_dir, root) = temp_log_root();
    let out = scotia([
        "--log-root",
        &root.to_string_lossy(),
        "run",
        "--agent",
        "generic",
        "--task",
        "missing binary",
        "--",
        "/definitely/does/not/exist/scotia_test_binary",
    ]);

    assert!(!out.status.success(), "expected failure for missing binary");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("failed to spawn") || stderr.contains("No such file"),
        "unexpected error for missing binary: {}",
        stderr
    );
}

#[cfg(unix)]
#[test]
fn unsafe_agent_binary_is_rejected() {
    use std::os::unix::fs::PermissionsExt;

    // An existing but group/other-writable agent binary must be refused before
    // exec, even though it is executable — a world-writable impostor on PATH
    // (or an explicit --agent-path) must never run.
    let (_dir, root) = temp_log_root();
    let tmp = TempDir::new().unwrap();
    let evil = tmp.path().join("evil_agent");
    fs::write(&evil, "#!/bin/sh\necho pwned\n").unwrap();
    fs::set_permissions(&evil, fs::Permissions::from_mode(0o777)).unwrap();

    let out = scotia([
        "--log-root",
        &root.to_string_lossy(),
        "run",
        "--agent",
        "generic",
        "--task",
        "unsafe binary",
        "--",
        &evil.to_string_lossy(),
    ]);

    assert!(!out.status.success(), "expected refusal for unsafe binary");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("is not a safe executable"),
        "expected safety rejection, got: {}",
        stderr
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn task_description_roundtrips(task in "[a-zA-Z0-9_][a-zA-Z0-9_ ]{0,199}") {
        let (_dir, root) = temp_log_root();
        let json = cli_run(&root, "generic", &task, "/bin/sh", &["-c", "echo roundtrip"]);

        let run_value: Value = serde_json::from_str(&fs::read_to_string(&json).unwrap()).unwrap();
        let stored_task = run_value["task"].as_str();
        prop_assert_eq!(stored_task, Some(task.as_str()));
    }
}

/// Persist a hand-built run to the temporary log store and return its JSON path.
fn persist_run(root: &Path, run: scotia::event::ScotiaRun) -> String {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let stored = rt
        .block_on(scotia::storage::store_run(
            &scotia::storage::StorageConfig {
                root: root.to_path_buf(),
                commit_to_git: false,
            },
            run,
        ))
        .expect("store_run should succeed");
    stored.json_path.to_string_lossy().into_owned()
}
