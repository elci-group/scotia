use std::env;
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
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
    env::current_exe().ok()?.parent().map(Path::to_path_buf)
}

/// Returns true when `path` is a regular file that is executable and not
/// writable by group/others. Vets binaries resolved from `PATH` before we
/// exec them, so a world-writable impostor earlier in `PATH` cannot be run.
fn is_safe_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        let mode = meta.mode();
        mode & 0o111 != 0 && mode & 0o022 == 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Locate the real `scotia` binary next to this shim, or search PATH (vetting
/// each candidate with [`is_safe_executable`]).
fn find_scotia_binary(shim_dir: &Path) -> PathBuf {
    let sibling = shim_dir.join("scotia");
    if is_safe_executable(&sibling) {
        return sibling;
    }
    if let Some(path_var) = env::var_os("PATH") {
        let shim_dir_canon =
            std::fs::canonicalize(shim_dir).unwrap_or_else(|_| shim_dir.to_path_buf());
        for dir in env::split_paths(&path_var) {
            // Do not resolve back into the shim directory.
            let canon = std::fs::canonicalize(&dir).unwrap_or(dir.clone());
            if canon == shim_dir_canon {
                continue;
            }
            let candidate = dir.join("scotia");
            if is_safe_executable(&candidate) {
                return candidate;
            }
        }
    }
    // Last resort: return the bare name and let the OS resolve it (the
    // subsequent exec will fail loudly if it cannot be found).
    PathBuf::from("scotia")
}

/// Find the real agent binary in PATH, excluding the shim directory so we
/// don't recurse, and vetting each candidate with [`is_safe_executable`].
fn find_real_binary(name: &str, shim_dir: &Path) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    let shim_dir_canon = std::fs::canonicalize(shim_dir).unwrap_or_else(|_| shim_dir.to_path_buf());

    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if !is_safe_executable(&candidate) {
            continue;
        }
        let canon = std::fs::canonicalize(&candidate).unwrap_or(candidate.clone());
        if canon != shim_dir_canon && canon.parent() != Some(&shim_dir_canon) {
            return Some(candidate);
        }
    }
    None
}

/// Returns true when `value` looks like an opt-in boolean ("1", "true", "yes").
fn env_flag(name: &str) -> bool {
    match env::var(name) {
        Ok(v) => matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes"),
        Err(_) => false,
    }
}

/// Resolve the real agent binary using the most restrictive source available:
///
/// 1. A configured pin in `agents.json` (keyed by `agent_kind`), vetted with
///    [`is_safe_executable`]. Pins let users hard-lock the exact binary the
///    shim is allowed to exec, defeating PATH impostors entirely.
/// 2. If `SCOTIA_NO_PATH_FALLBACK` is set and no usable pin exists, refuse
///    rather than searching PATH (caller requested explicit pinning only).
/// 3. Otherwise, search PATH (excluding the shim dir) via [`find_real_binary`].
fn resolve_real_binary(invoked_as: &str, agent_kind: &str, shim_dir: &Path) -> Option<PathBuf> {
    let pins = scotia::shim::load_agent_pins();
    if let Some(pinned) = pins.get(agent_kind).map(PathBuf::from) {
        if is_safe_executable(&pinned) {
            return Some(pinned);
        }
        eprintln!(
            "scotia-shim: pinned path for '{}' ({}) is not a safe executable; ignoring pin",
            agent_kind,
            pinned.display()
        );
    }

    if env_flag("SCOTIA_NO_PATH_FALLBACK") {
        return None;
    }

    find_real_binary(invoked_as, shim_dir)
}

fn main() {
    let args: Vec<OsString> = env::args_os().collect();
    let invoked_as = args
        .first()
        .map(std::path::Path::new)
        .and_then(|p| p.file_stem())
        .and_then(|s| s.to_str())
        .unwrap_or("scotia-shim");

    // Find our shim directory early so we can resolve siblings and avoid recursion.
    let shim_dir = shim_dir().unwrap_or_else(|| PathBuf::from("."));

    // If invoked under a name we don't recognise, refuse rather than executing
    // an arbitrary caller-supplied program (argv[1]). This shim exists solely
    // to dispatch to known agent binaries.
    let Some(agent_kind) = agent_kind_for_name(invoked_as) else {
        eprintln!(
            "scotia-shim: '{}' is not a recognised agent name; refusing to run",
            invoked_as
        );
        std::process::exit(127);
    };

    // Resolve the real agent binary (e.g. /usr/local/bin/claude). Pins in
    // agents.json take precedence; PATH search can be disabled with
    // SCOTIA_NO_PATH_FALLBACK to refuse un-pinned execution.
    let real_binary = resolve_real_binary(invoked_as, agent_kind, &shim_dir).unwrap_or_else(|| {
        if env_flag("SCOTIA_NO_PATH_FALLBACK") {
            eprintln!(
                "scotia-shim: no usable pin for '{}' and SCOTIA_NO_PATH_FALLBACK is set; refusing",
                agent_kind
            );
        } else {
            eprintln!(
                "scotia-shim: could not find real binary for '{}' in PATH",
                invoked_as
            );
        }
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
