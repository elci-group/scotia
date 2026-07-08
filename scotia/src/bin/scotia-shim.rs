use std::env;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Supported agent binary names and the Scotia AgentKind they map to.
fn agent_kind_for_name(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    match lower.as_str() {
        "kimi" | "kimi-code" | "kimi_code" => Some("kimi-code"),
        "agy" => Some("agy"),
        "cosine" => Some("cosine"),
        "codex" | "codex-cli" | "codex_cli" => Some("codex"),
        "claude" | "claude-code" | "claude_code" => Some("claude-code"),
        "opencode" => Some("opencode"),
        _ => None,
    }
}

/// Find the directory this shim binary lives in.
fn shim_dir() -> Option<PathBuf> {
    env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

/// Locate the real `scotia` binary next to this shim, or fall back to PATH.
fn find_scotia_binary(shim_dir: &Path) -> PathBuf {
    let sibling = shim_dir.join("scotia");
    if sibling.exists() {
        return sibling;
    }
    PathBuf::from("scotia")
}

/// Find the real agent binary in PATH, excluding the shim directory so we don't recurse.
fn find_real_binary(name: &str, shim_dir: &Path) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    let shim_dir_canon = std::fs::canonicalize(shim_dir).unwrap_or_else(|_| shim_dir.to_path_buf());

    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.exists() {
            let canon = std::fs::canonicalize(&candidate).unwrap_or(candidate.clone());
            if canon != shim_dir_canon && canon.parent() != Some(&shim_dir_canon) {
                return Some(candidate);
            }
        }
    }
    None
}

fn main() {
    let args: Vec<OsString> = env::args_os().collect();
    let invoked_as = args
        .first()
        .map(|s| std::path::Path::new(s))
        .and_then(|p| p.file_stem())
        .and_then(|s| s.to_str())
        .unwrap_or("scotia-shim");

    // Find our shim directory early so we can resolve siblings and avoid recursion.
    let shim_dir = shim_dir().unwrap_or_else(|| PathBuf::from("."));

    // If invoked directly by a name we don't recognise, just pass through unchanged.
    let Some(agent_kind) = agent_kind_for_name(invoked_as) else {
        let program = args
            .get(1)
            .cloned()
            .unwrap_or_else(|| OsString::from("scotia"));
        let rest = args.into_iter().skip(2).collect::<Vec<_>>();
        let err = Command::new(program).args(rest).exec();
        eprintln!("scotia-shim: pass-through exec failed: {}", err);
        std::process::exit(126);
    };

    // Resolve the real agent binary (e.g. /usr/local/bin/claude).
    let real_binary = find_real_binary(invoked_as, &shim_dir).unwrap_or_else(|| {
        eprintln!(
            "scotia-shim: could not find real binary for '{}' in PATH",
            invoked_as
        );
        std::process::exit(127);
    });

    let scotia = find_scotia_binary(&shim_dir);

    // Build: scotia run --agent <kind> -- <real-binary> [original args...]
    let mut scotia_args: Vec<OsString> = vec![
        OsString::from("run"),
        OsString::from("--agent"),
        OsString::from(agent_kind),
        OsString::from("--"),
        real_binary.into_os_string(),
    ];
    scotia_args.extend(args.into_iter().skip(1));

    let err = Command::new(scotia).args(scotia_args).exec();
    eprintln!("scotia-shim: failed to exec scotia: {}", err);
    std::process::exit(126);
}
