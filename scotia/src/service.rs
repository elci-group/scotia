use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Platform-specific service manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServicePlatform {
    Systemd,
    Launchd,
    Unknown,
}

impl ServicePlatform {
    pub fn detect() -> Self {
        if cfg!(target_os = "macos") {
            ServicePlatform::Launchd
        } else if cfg!(target_os = "linux") {
            ServicePlatform::Systemd
        } else {
            ServicePlatform::Unknown
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ServicePlatform::Systemd => "systemd",
            ServicePlatform::Launchd => "launchd",
            ServicePlatform::Unknown => "unknown",
        }
    }
}

/// Result of installing or uninstalling a service.
#[derive(Debug)]
pub struct ServiceResult {
    pub platform: ServicePlatform,
    pub installed_path: Option<PathBuf>,
    pub output: String,
}

/// Install the daemon as a user service.
pub fn install_service() -> Result<ServiceResult> {
    let platform = ServicePlatform::detect();
    match platform {
        ServicePlatform::Systemd => install_systemd(),
        ServicePlatform::Launchd => install_launchd(),
        ServicePlatform::Unknown => anyhow::bail!("unsupported platform for service installation"),
    }
}

/// Uninstall the daemon user service.
pub fn uninstall_service() -> Result<ServiceResult> {
    let platform = ServicePlatform::detect();
    match platform {
        ServicePlatform::Systemd => uninstall_systemd(),
        ServicePlatform::Launchd => uninstall_launchd(),
        ServicePlatform::Unknown => {
            anyhow::bail!("unsupported platform for service uninstallation")
        }
    }
}

fn install_systemd() -> Result<ServiceResult> {
    let source = bundled_service_file("scotiad.service")?;
    let user_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("systemd")
        .join("user");
    fs::create_dir_all(&user_dir)?;
    let dest = user_dir.join("scotiad.service");

    let service_text = fs::read_to_string(&source)?;
    let expanded = expand_service_template(&service_text);
    fs::write(&dest, expanded)?;

    let mut output = String::new();
    output.push_str(&run_cmd("systemctl", &["--user", "daemon-reload"])?);
    output.push('\n');
    output.push_str(&run_cmd(
        "systemctl",
        &["--user", "enable", "scotiad.service"],
    )?);
    output.push('\n');
    output.push_str(&run_cmd(
        "systemctl",
        &["--user", "start", "scotiad.service"],
    )?);

    Ok(ServiceResult {
        platform: ServicePlatform::Systemd,
        installed_path: Some(dest),
        output,
    })
}

fn uninstall_systemd() -> Result<ServiceResult> {
    let dest = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("systemd/user/scotiad.service");

    let mut output = String::new();
    output.push_str(
        &run_cmd("systemctl", &["--user", "stop", "scotiad.service"]).unwrap_or_default(),
    );
    output.push('\n');
    output.push_str(
        &run_cmd("systemctl", &["--user", "disable", "scotiad.service"]).unwrap_or_default(),
    );
    output.push('\n');

    if dest.exists() {
        fs::remove_file(&dest)?;
        output.push_str("Removed service file.");
    }
    output.push('\n');
    output.push_str(&run_cmd("systemctl", &["--user", "daemon-reload"]).unwrap_or_default());

    Ok(ServiceResult {
        platform: ServicePlatform::Systemd,
        installed_path: None,
        output,
    })
}

fn install_launchd() -> Result<ServiceResult> {
    let source = bundled_service_file("com.scotia.scotiad.plist")?;
    let agents_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("Library/LaunchAgents");
    fs::create_dir_all(&agents_dir)?;
    let dest = agents_dir.join("com.scotia.scotiad.plist");

    let plist_text = fs::read_to_string(&source)?;
    let expanded = expand_service_template(&plist_text);
    fs::write(&dest, expanded)?;

    let output = run_cmd("launchctl", &["load", dest.to_string_lossy().as_ref()])?;

    Ok(ServiceResult {
        platform: ServicePlatform::Launchd,
        installed_path: Some(dest),
        output,
    })
}

fn uninstall_launchd() -> Result<ServiceResult> {
    let dest = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("Library/LaunchAgents/com.scotia.scotiad.plist");

    let mut output = String::new();
    if dest.exists() {
        output.push_str(
            &run_cmd("launchctl", &["unload", dest.to_string_lossy().as_ref()]).unwrap_or_default(),
        );
        fs::remove_file(&dest)?;
        output.push('\n');
        output.push_str("Removed plist file.");
    }

    Ok(ServiceResult {
        platform: ServicePlatform::Launchd,
        installed_path: None,
        output,
    })
}

fn bundled_service_file(name: &str) -> Result<PathBuf> {
    // When running from cargo build, the deploy directory is next to the manifest.
    let manifest_dir = std::env!("CARGO_MANIFEST_DIR");
    let path = PathBuf::from(manifest_dir).join("deploy").join(name);
    if path.exists() {
        return Ok(path);
    }
    // Fallback: assume the binary is in the same directory as the deploy folder (unusual).
    let exe_dir = std::env::current_exe()?
        .parent()
        .map(PathBuf::from)
        .unwrap_or_default();
    let fallback = exe_dir.join("deploy").join(name);
    if fallback.exists() {
        return Ok(fallback);
    }
    anyhow::bail!("could not find bundled service file {}", name)
}

fn expand_service_template(text: &str) -> String {
    let home = dirs::home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "~".to_string());
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "scotiad".to_string());

    text.replace("%h", &home)
        .replace("/usr/local/bin/scotiad", &exe)
        .replace("/Users/%u", &home)
}

fn run_cmd(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to run {} {:?}", program, args))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        anyhow::bail!("{} {:?} failed: {} {}", program, args, stdout, stderr);
    }
    Ok(format!("{}{}", stdout, stderr).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_platform() {
        let platform = ServicePlatform::detect();
        if cfg!(target_os = "macos") {
            assert_eq!(platform, ServicePlatform::Launchd);
        } else if cfg!(target_os = "linux") {
            assert_eq!(platform, ServicePlatform::Systemd);
        }
    }

    #[test]
    fn expand_replaces_home_placeholder() {
        let expanded = expand_service_template("ExecStart=%h/.local/bin/scotiad");
        assert!(!expanded.contains('%'));
    }

    #[test]
    fn bundled_service_file_finds_systemd_unit() {
        let path = bundled_service_file("scotiad.service").unwrap();
        assert!(path.exists());
    }
}
