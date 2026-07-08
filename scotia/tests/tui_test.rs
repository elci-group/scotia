use scotia::event::AgentKind;
use std::ffi::OsString;

#[test]
fn detect_harnesses_with_path_finds_fake_binary() {
    let temp = tempfile::tempdir().unwrap();
    let bin_dir = temp.path();

    // Create a fake "claude" executable.
    let fake_claude = bin_dir.join("claude");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(&fake_claude, "#!/bin/sh\necho fake").unwrap();
        let mut perms = std::fs::metadata(&fake_claude).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&fake_claude, perms).unwrap();
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&fake_claude, "").unwrap();
    }

    let path = OsString::from(bin_dir.as_os_str());
    let harnesses = scotia::tui::detect_harnesses_with_path(path.as_os_str());

    assert!(
        harnesses.iter().any(|h| h.agent == AgentKind::ClaudeCode),
        "expected ClaudeCode harness to be detected"
    );
}

#[test]
fn detect_harnesses_with_path_empty_when_no_binaries() {
    let temp = tempfile::tempdir().unwrap();
    let path = OsString::from(temp.path().as_os_str());
    let harnesses = scotia::tui::detect_harnesses_with_path(path.as_os_str());
    assert!(harnesses.is_empty());
}
