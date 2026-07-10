use anyhow::{Context, Result};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Installation scope for startup services.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum InstallScope {
    /// Install only for the current user.
    User,
    /// Install system-wide (requires root/admin).
    System,
}

impl InstallScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            InstallScope::User => "user",
            InstallScope::System => "system",
        }
    }
}

/// Options controlling a GUI-driven installation.
#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub scope: InstallScope,
    pub autostart: bool,
    pub install_shims: bool,
    pub bin_dir: PathBuf,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            scope: InstallScope::User,
            autostart: true,
            install_shims: true,
            bin_dir: default_bin_dir(),
        }
    }
}

pub fn default_bin_dir() -> PathBuf {
    if cfg!(target_os = "windows") {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
            .join("Scotia")
            .join("bin")
    } else if cfg!(target_os = "macos") {
        PathBuf::from("/usr/local/bin")
    } else {
        dirs::home_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join(".local")
            .join("bin")
    }
}

/// Apply an installation from the current build artifacts.
///
/// The installer is expected to have already copied the three binaries
/// (`scotia`, `scotia-shim`, `scotiad`) into `options.bin_dir`. This routine
/// then wires up services, shims, and PATH entries.
pub fn apply_install(options: &InstallOptions) -> Result<()> {
    fs::create_dir_all(&options.bin_dir)
        .with_context(|| format!("failed to create bin dir {}", options.bin_dir.display()))?;

    if options.install_shims {
        let shim_dir = if cfg!(target_os = "windows") {
            options.bin_dir.clone()
        } else {
            default_shim_dir()
        };
        install_shims(&shim_dir, &options.bin_dir)?;
    }

    if options.autostart {
        install_service(options.scope, &options.bin_dir)?;
    }

    Ok(())
}

fn install_shims(shim_dir: &Path, bin_dir: &Path) -> Result<()> {
    fs::create_dir_all(shim_dir)?;

    let shim_source = bin_dir.join(if cfg!(target_os = "windows") {
        "scotia-shim.exe"
    } else {
        "scotia-shim"
    });

    for name in crate::shim::DEFAULT_AGENT_NAMES {
        if cfg!(target_os = "windows") {
            let batch = shim_dir.join(format!("{}.cmd", name));
            if batch.exists() {
                continue;
            }
            fs::write(
                &batch,
                format!(
                    "@echo off\n\"{}\" run --agent {} -- %*\n",
                    shim_source.display(),
                    name
                ),
            )?;
        } else {
            let link = shim_dir.join(name);
            if link.exists() || link.symlink_metadata().is_ok() {
                continue;
            }
            symlink(&shim_source, &link)
                .with_context(|| format!("failed to create shim {}", link.display()))?;
        }
    }

    if !cfg!(target_os = "windows") {
        crate::shim::update_shell_path(shim_dir)?;
    }
    Ok(())
}

fn default_shim_dir() -> PathBuf {
    crate::shim::default_shim_dir()
}

fn install_service(scope: InstallScope, bin_dir: &Path) -> Result<()> {
    if cfg!(target_os = "linux") {
        install_linux_service(scope, bin_dir)
    } else if cfg!(target_os = "macos") {
        install_macos_service(scope, bin_dir)
    } else if cfg!(target_os = "windows") {
        install_windows_service(scope, bin_dir)
    } else {
        anyhow::bail!("unsupported platform for service installation")
    }
}

/// Escape a path for use as a systemd `ExecStart=` executable.
///
/// systemd tokenises `ExecStart` itself (it does not invoke a shell): wrapping
/// the path in double quotes and escaping embedded backslashes and quotes is
/// the documented way to tolerate spaces. Newlines/carriage returns are
/// rejected outright because they would otherwise let a crafted path inject
/// additional `[Service]` directives.
fn systemd_escape_exec(s: &str) -> Result<String> {
    if s.contains('\n') || s.contains('\r') {
        anyhow::bail!(
            "refusing to embed a path containing a newline in a unit file: {}",
            s
        );
    }
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    Ok(format!("\"{}\"", escaped))
}

/// Escape text for inclusion in an XML plist `<string>` element.
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Best-effort check for an elevated (root/admin) process.
fn is_elevated() -> bool {
    #[cfg(unix)]
    {
        // SAFETY: geteuid() is always safe to call and cannot fail.
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// Hardening directives applied to generated per-user systemd units.
///
/// The daemon only needs Unix-socket IPC and D-Bus notifications, so we can
/// lock it down tightly. Home is left writable (the daemon writes its pid/log
/// under the per-user state dir); everything under /usr and /etc is read-only.
///
/// `CapabilityBoundingSet=` drops every capability (the daemon needs none) and
/// `SystemCallFilter=@system-service` allow-lists the syscalls a typical
/// event-loop service needs. Both require systemd 248+ (the `@system-service`
/// set was introduced in 248); older releases ignore unknown filter sets and
/// fall back to the remaining directives.
const SYSTEMD_HARDENING: &str = "\
NoNewPrivileges=true
PrivateTmp=true
PrivateDevices=true
ProtectSystem=full
RestrictNamespaces=true
RestrictRealtime=true
RestrictSUIDSGID=true
LockPersonality=true
MemoryDenyWriteExecute=true
RestrictAddressFamilies=AF_UNIX
CapabilityBoundingSet=
SystemCallFilter=@system-service
SystemCallErrorNumber=EPERM";

fn install_linux_service(scope: InstallScope, bin_dir: &Path) -> Result<()> {
    if is_elevated() {
        anyhow::bail!(
            "the Scotia daemon is a per-user process and must not run as root. \
             Binaries may be placed system-wide by the package, but the \
             autostart service must be installed as the target user, e.g. \
             `scotia daemon install-service` (without sudo)."
        );
    }
    if scope == InstallScope::System {
        eprintln!(
            "note: the Scotia daemon is per-user; installing the autostart \
             service for the current user (a system-wide daemon is not supported)."
        );
    }

    let scotiad = bin_dir.join("scotiad");
    let exec_start = systemd_escape_exec(&scotiad.display().to_string())?;
    let service_text = format!(
        "[Unit]
Description=Scotia daemon
After=default.target

[Service]
Type=simple
ExecStart={exec_start}
Restart=on-failure
RestartSec=5
{hardening}

[Install]
WantedBy=default.target
",
        exec_start = exec_start,
        hardening = SYSTEMD_HARDENING,
    );

    let dir = dirs::config_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join(".config")
        })
        .join("systemd/user");
    fs::create_dir_all(&dir)?;
    let path = dir.join("scotiad.service");
    fs::write(&path, service_text)?;
    run("systemctl", &["--user", "daemon-reload"])?;
    run("systemctl", &["--user", "enable", "scotiad.service"])?;
    run("systemctl", &["--user", "start", "scotiad.service"])?;
    Ok(())
}

fn install_macos_service(scope: InstallScope, bin_dir: &Path) -> Result<()> {
    if is_elevated() {
        anyhow::bail!(
            "the Scotia daemon is a per-user process and must not run as root. \
             Install the LaunchAgent as the target user (without sudo)."
        );
    }
    if scope == InstallScope::System {
        eprintln!(
            "note: the Scotia daemon is per-user; installing a LaunchAgent for \
             the current user (a LaunchDaemon is not supported)."
        );
    }

    let scotiad = bin_dir.join("scotiad");
    let home = dirs::home_dir().unwrap_or_else(std::env::temp_dir);
    let plist_text = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.scotia.scotiad</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
</dict>
</plist>
"#,
        xml_escape(&scotiad.display().to_string())
    );

    let dir = home.join("Library/LaunchAgents");
    fs::create_dir_all(&dir)?;
    let path = dir.join("com.scotia.scotiad.plist");
    fs::write(&path, plist_text)?;
    run("launchctl", &["load", path.to_string_lossy().as_ref()])?;
    Ok(())
}

fn install_windows_service(scope: InstallScope, bin_dir: &Path) -> Result<()> {
    let scotiad = bin_dir.join("scotiad.exe");
    let scotiad_quoted = format!("\"{}\"", scotiad.display());

    match scope {
        InstallScope::User => {
            // Per-user autostart via the Run registry key.
            run(
                "reg.exe",
                &[
                    "add",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v",
                    "ScotiaDaemon",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &scotiad_quoted,
                    "/f",
                ],
            )?;
        }
        InstallScope::System => {
            // Refuse to install a SYSTEM-level daemon: the daemon is a per-user
            // process and must not run as LocalSystem. Binaries may still be
            // placed system-wide; users enable autostart per-account.
            anyhow::bail!(
                "the Scotia daemon is per-user; refusing to install a SYSTEM \
                 service. Run `scotia daemon install-service` as the target user."
            );
        }
    }
    Ok(())
}

fn run(program: &str, args: &[&str]) -> Result<()> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to run {} {:?}", program, args))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{} {:?} failed: {}", program, args, stderr);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_options_are_user_scope() {
        let opts = InstallOptions::default();
        assert_eq!(opts.scope, InstallScope::User);
        assert!(opts.autostart);
        assert!(opts.install_shims);
    }

    #[test]
    fn install_options_can_be_serialized() {
        let opts = InstallOptions {
            scope: InstallScope::System,
            autostart: false,
            install_shims: false,
            bin_dir: PathBuf::from("/opt/scotia/bin"),
        };
        assert_eq!(opts.scope.as_str(), "system");
    }

    #[test]
    fn install_shims_creates_links_in_temp_dir() {
        let tmp = TempDir::new().unwrap();
        let bin_dir = tmp.path().join("bin");
        let shim_dir = tmp.path().join("shims");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::write(bin_dir.join("scotia-shim"), b"").unwrap();
        install_shims(&shim_dir, &bin_dir).unwrap();
        assert!(shim_dir.join("kimi").exists() || shim_dir.join("kimi.cmd").exists());
    }

    // A newline in a path would let an attacker append `[Service]` directives
    // to the generated unit; it must be rejected outright.
    #[test]
    fn systemd_escape_exec_rejects_newlines() {
        assert!(systemd_escape_exec("/bin/evil\n[Service]").is_err());
        assert!(systemd_escape_exec("/bin/evil\r\n[Service]").is_err());
    }

    // Ordinary paths are wrapped in double quotes; embedded quotes/backslashes
    // are escaped so systemd tokenises the path as a single argument.
    #[test]
    fn systemd_escape_exec_quotes_and_escapes_path() {
        let out = systemd_escape_exec("/usr/local/bin/scotiad").unwrap();
        assert!(out.starts_with('"') && out.ends_with('"'));
        assert_eq!(out.len(), "/usr/local/bin/scotiad".len() + 2);

        let escaped = systemd_escape_exec(r#"/opt/a"b\c"#).unwrap();
        assert!(escaped.contains(r#"\""#), "embedded quote must be escaped");
        assert!(
            escaped.contains(r"\\"),
            "embedded backslash must be escaped"
        );
    }

    #[test]
    fn xml_escape_escapes_all_significant_chars() {
        assert_eq!(xml_escape(r#"<a>&"'"#), "&lt;a&gt;&amp;&quot;&apos;");
    }
}
