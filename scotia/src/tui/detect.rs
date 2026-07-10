//! PATH scanning for installed agent harnesses.
//!
//! Pure, I/O-light detection lifted out of `tui.rs` so it can be tested and
//! reused without the terminal machinery. Re-exported from `tui` to keep the
//! public surface (`tui::Harness`, `tui::detect_harnesses*`) unchanged.

use std::ffi::OsStr;
use std::path::PathBuf;

use crate::event::AgentKind;

/// A harness that Scotia can wrap and observe.
#[derive(Debug, Clone)]
pub struct Harness {
    pub display_name: String,
    pub agent: AgentKind,
    pub binary: PathBuf,
    pub args: Vec<String>,
}

impl Harness {
    pub fn new(display_name: impl Into<String>, agent: AgentKind, binary: PathBuf) -> Self {
        Self {
            display_name: display_name.into(),
            agent,
            binary,
            args: Vec::new(),
        }
    }
}

/// Detect installed agent harnesses by scanning PATH for known binaries.
pub fn detect_harnesses() -> Vec<Harness> {
    detect_harnesses_with_path(std::env::var_os("PATH").unwrap_or_default().as_os_str())
}

/// Detect harnesses using a provided PATH-style string (testable entry point).
pub fn detect_harnesses_with_path(path: &OsStr) -> Vec<Harness> {
    let candidates = vec![
        (
            "claude-code",
            AgentKind::ClaudeCode,
            vec!["claude", "claude-code", "claude_code"],
        ),
        (
            "kimi-code",
            AgentKind::KimiCode,
            vec!["kimi", "kimi-code", "kimi_code"],
        ),
        (
            "codex",
            AgentKind::Codex,
            vec!["codex", "codex-cli", "codex_cli"],
        ),
        ("agy", AgentKind::Agy, vec!["agy"]),
        ("cosine", AgentKind::Cosine, vec!["cosine"]),
        ("opencode", AgentKind::Opencode, vec!["opencode"]),
    ];

    let mut harnesses = Vec::new();

    for (display, agent, names) in candidates {
        for name in names {
            if let Some(bin) = find_in_path(name, path) {
                harnesses.push(Harness::new(display, agent, bin));
                break;
            }
        }
    }

    harnesses
}

fn find_in_path(name: &str, path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|full| crate::shim::is_safe_executable(full))
}
