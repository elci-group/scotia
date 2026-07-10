//! Shim lifecycle subcommands: install-shims, uninstall-shims. Extracted from
//! `cli.rs` to keep the top-level dispatcher thin and co-locate shim behaviour.

use crate::notify::{Notifier, shims_installed, shims_uninstalled};
use crate::shim::{
    DEFAULT_AGENT_NAMES, default_shim_dir, detect_aliases, find_scotia_shim_binary, install_shims,
    remove_shell_path, uninstall_shims, update_shell_path,
};
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

pub fn install(shim_dir: Option<PathBuf>, notifier: &Arc<dyn Notifier>) -> Result<()> {
    let shim_dir = shim_dir.unwrap_or_else(default_shim_dir);
    let scotia_shim = find_scotia_shim_binary()?;
    let aliases = detect_aliases(DEFAULT_AGENT_NAMES);
    if !aliases.is_empty() {
        eprintln!("Detected shell aliases that may shadow shims:");
        for a in &aliases {
            eprintln!("  - {}", a);
        }
        eprintln!("Consider removing them or re-sourcing your shell config.");
    }
    let result = install_shims(&shim_dir, &scotia_shim, DEFAULT_AGENT_NAMES)?;
    update_shell_path(&shim_dir)?;
    println!(
        "Installed {} shims to {}",
        result.created.len(),
        shim_dir.display()
    );
    if !result.collisions.is_empty() {
        eprintln!("Warning: existing binaries earlier in PATH:");
        for c in &result.collisions {
            eprintln!("  - {}", c);
        }
    }
    notifier.notify(shims_installed(result.created.len()))?;
    Ok(())
}

pub fn uninstall(shim_dir: Option<PathBuf>, notifier: &Arc<dyn Notifier>) -> Result<()> {
    let shim_dir = shim_dir.unwrap_or_else(default_shim_dir);
    let removed = uninstall_shims(&shim_dir, DEFAULT_AGENT_NAMES)?;
    remove_shell_path(&shim_dir)?;
    println!(
        "Removed {} shims from {}",
        removed.len(),
        shim_dir.display()
    );
    notifier.notify(shims_uninstalled())?;
    Ok(())
}
