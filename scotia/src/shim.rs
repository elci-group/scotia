use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::{Path, PathBuf};

/// Default set of agent binary names to shim.
pub const DEFAULT_AGENT_NAMES: &[&str] = &[
    "kimi",
    "kimi-code",
    "agy",
    "cosine",
    "codex",
    "claude",
    "claude-code",
    "opencode",
];

/// Where shims are installed.
pub fn default_shim_dir() -> PathBuf {
    // Prefer the per-user data dir; if that is unavailable, derive it from the
    // home directory; as a last resort use the process temp dir. Never emit a
    // literal `~` component (it is not expanded by `PathBuf`).
    let base = dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local").join("share")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("scotia").join("shims")
}

/// Result of installing shims.
#[derive(Debug, Default)]
pub struct InstallResult {
    pub created: Vec<String>,
    pub skipped: Vec<String>,
    pub collisions: Vec<String>,
}

/// Returns true when `path` is a regular file that is executable and is not
/// writable by group/others (i.e. it could only have been modified by its
/// owner). Used to vet binaries resolved from `PATH` before we execute them,
/// so a world-writable impostor earlier in `PATH` cannot be run.
pub fn is_safe_executable(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        let mode = meta.mode();
        // Executable by someone, and not writable by group or others.
        mode & 0o111 != 0 && mode & 0o022 == 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Install shims by symlinking agent names to the scotia-shim binary.
pub fn install_shims(
    shim_dir: &Path,
    scotia_shim: &Path,
    agents: &[&str],
) -> Result<InstallResult> {
    fs::create_dir_all(shim_dir)
        .with_context(|| format!("failed to create shim directory {}", shim_dir.display()))?;

    let shim_dir_canon = fs::canonicalize(shim_dir).unwrap_or_else(|_| shim_dir.to_path_buf());
    let path_entries = path_entries();
    let mut result = InstallResult::default();

    for name in agents {
        let link_path = shim_dir.join(name);

        if link_path.exists() || link_path.symlink_metadata().is_ok() {
            result.skipped.push((*name).to_string());
            continue;
        }

        // Detect if a real binary with this name appears earlier in PATH.
        if let Some(existing) = find_in_path(name, &path_entries) {
            let existing_canon = fs::canonicalize(&existing).unwrap_or(existing.clone());
            if existing_canon != shim_dir_canon && existing_canon.parent() != Some(&shim_dir_canon)
            {
                result
                    .collisions
                    .push(format!("{} -> {}", name, existing.display()));
            }
        }

        #[cfg(unix)]
        symlink(scotia_shim, &link_path)
            .with_context(|| format!("failed to create shim symlink for {}", name))?;
        result.created.push((*name).to_string());
    }

    Ok(result)
}

/// Remove all shims we created.
pub fn uninstall_shims(shim_dir: &Path, agents: &[&str]) -> Result<Vec<String>> {
    let mut removed = Vec::new();
    for name in agents {
        let link_path = shim_dir.join(name);
        if let Ok(meta) = link_path.symlink_metadata()
            && meta.file_type().is_symlink()
        {
            fs::remove_file(&link_path)
                .with_context(|| format!("failed to remove shim {}", link_path.display()))?;
            removed.push((*name).to_string());
        }
    }
    Ok(removed)
}

/// Update shell rc files to prepend the shim directory to PATH.
pub fn update_shell_path(shim_dir: &Path) -> Result<Vec<PathBuf>> {
    let block = path_block(shim_dir);
    let mut updated = Vec::new();

    for rc in shell_rc_files() {
        let contents = fs::read_to_string(&rc).unwrap_or_default();
        if contents.contains(&block) {
            continue;
        }
        fs::write(&rc, format!("{}\n{}", contents.trim_end(), block))
            .with_context(|| format!("failed to update {}", rc.display()))?;
        updated.push(rc);
    }

    Ok(updated)
}

/// Remove the Scotia PATH block from shell rc files.
pub fn remove_shell_path(shim_dir: &Path) -> Result<Vec<PathBuf>> {
    let block = path_block(shim_dir);
    let mut updated = Vec::new();

    for rc in shell_rc_files() {
        let contents = fs::read_to_string(&rc).unwrap_or_default();
        if !contents.contains(&block) {
            continue;
        }
        let cleaned = contents.replace(&block, "");
        fs::write(&rc, cleaned).with_context(|| format!("failed to update {}", rc.display()))?;
        updated.push(rc);
    }

    Ok(updated)
}

fn path_block(shim_dir: &Path) -> String {
    format!(
        "# >>> Scotia shims >>>\nexport PATH=\"{}:$PATH\"\n# <<< Scotia shims <<<",
        shim_dir.display()
    )
}

fn shell_rc_files() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_else(std::env::temp_dir);
    vec![
        home.join(".bashrc"),
        home.join(".zshrc"),
        home.join(".config/fish/config.fish"),
    ]
    .into_iter()
    .filter(|p| p.exists())
    .collect()
}

fn path_entries() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default()
}

fn find_in_path(name: &str, entries: &[PathBuf]) -> Option<PathBuf> {
    for dir in entries {
        let candidate = dir.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// Locate the scotia-shim binary. Prefers the cargo-built sibling, then PATH
/// (vetting each PATH candidate with [`is_safe_executable`]).
pub fn find_scotia_shim_binary() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let sibling = exe
        .parent()
        .map(|p| p.join("scotia-shim"))
        .filter(|p| p.exists());
    if let Some(p) = sibling {
        return Ok(p);
    }
    for dir in path_entries() {
        let candidate = dir.join("scotia-shim");
        if is_safe_executable(&candidate) {
            return Ok(candidate);
        }
    }
    anyhow::bail!("could not find scotia-shim binary in PATH or next to scotia")
}

/// Detect shell aliases that would shadow the shims.
pub fn detect_aliases(agents: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    for shell in ["bash", "zsh", "fish"] {
        let output = std::process::Command::new(shell)
            .args(["-c", "alias"])
            .output();
        if let Ok(out) = output {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                for name in agents {
                    if line.starts_with(&format!("alias {}=", name))
                        || line.starts_with(&format!("{}: aliased to", name))
                    {
                        found.push(format!("{}: {}", shell, line.trim()));
                    }
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn install_and_uninstall_symlinks() {
        let tmp = TempDir::new().unwrap();
        let shim_dir = tmp.path().join("shims");
        let fake_shim = tmp.path().join("scotia-shim");
        fs::write(&fake_shim, b"").unwrap();

        let result = install_shims(&shim_dir, &fake_shim, &["claude", "kimi"]).unwrap();
        assert_eq!(result.created.len(), 2);
        assert!(shim_dir.join("claude").exists());

        let removed = uninstall_shims(&shim_dir, &["claude", "kimi"]).unwrap();
        assert_eq!(removed.len(), 2);
        assert!(!shim_dir.join("claude").exists());
    }

    #[test]
    fn path_block_contains_shim_dir() {
        let block = path_block(Path::new("/home/sal/.local/share/scotia/shims"));
        assert!(block.contains("/home/sal/.local/share/scotia/shims"));
        assert!(block.contains("Scotia shims"));
    }
}

// ---------------------------------------------------------------------------
// Pinned agent paths (L4 hardening)
// ---------------------------------------------------------------------------

/// Path to the optional agent-pin file. Override with `SCOTIA_AGENT_PINS` for
/// testing or custom locations.
pub fn agent_pins_path() -> PathBuf {
    if let Some(p) = std::env::var_os("SCOTIA_AGENT_PINS") {
        return PathBuf::from(p);
    }
    dirs::config_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join(".config")
        })
        .join("scotia")
        .join("agents.json")
}

/// Load agent pins from an arbitrary path. The file is a small JSON object
/// mapping agent kind to an absolute binary path, e.g.
/// `{ "claude-code": "/usr/local/bin/claude" }`. Missing or malformed files
/// yield an empty map (pins are strictly optional).
pub fn load_agent_pins_from(path: &Path) -> HashMap<String, String> {
    let Ok(text) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    serde_json::from_str::<HashMap<String, String>>(&text).unwrap_or_default()
}

/// Load agent pins from the default location.
pub fn load_agent_pins() -> HashMap<String, String> {
    load_agent_pins_from(&agent_pins_path())
}

/// Return the pinned absolute path for `kind`, if one is configured.
pub fn pinned_agent_path(kind: crate::event::AgentKind) -> Option<PathBuf> {
    load_agent_pins().get(kind.as_str()).map(PathBuf::from)
}

#[cfg(test)]
mod pins_tests {
    use super::*;

    #[test]
    fn loads_pins_from_json_object() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("agents.json");
        fs::write(
            &path,
            r#"{ "claude-code": "/usr/local/bin/claude", "kimi-code": "/opt/kimi" }"#,
        )
        .unwrap();

        let pins = load_agent_pins_from(&path);
        assert_eq!(
            pins.get("claude-code").map(String::as_str),
            Some("/usr/local/bin/claude")
        );
        assert_eq!(pins.get("kimi-code").map(String::as_str), Some("/opt/kimi"));
        assert_eq!(pins.get("codex"), None);
    }

    #[test]
    fn missing_or_malformed_pins_yields_empty() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(load_agent_pins_from(&tmp.path().join("nope.json")).is_empty());

        let bad = tmp.path().join("bad.json");
        fs::write(&bad, "not json").unwrap();
        assert!(load_agent_pins_from(&bad).is_empty());
    }
}
