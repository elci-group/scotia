use crate::event::AgentKind;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Maritime severity levels for the Nova Scotia notification theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationLevel {
    /// Background state update.
    LightFlurries,
    /// Run finished cleanly.
    HarbourClear,
    /// Long-running run still active.
    IceField,
    /// Run finished with errors or retries.
    NoreasterWarning,
    /// Agent crashed or Scotia failed to wrap.
    Mayday,
}

impl NotificationLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            NotificationLevel::LightFlurries => "Light flurries",
            NotificationLevel::HarbourClear => "Harbour clear",
            NotificationLevel::IceField => "In the ice field",
            NotificationLevel::NoreasterWarning => "Nor'easter warning",
            NotificationLevel::Mayday => "Mayday",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            NotificationLevel::LightFlurries => "❄️",
            NotificationLevel::HarbourClear => "🌊",
            NotificationLevel::IceField => "🧊",
            NotificationLevel::NoreasterWarning => "🌨️",
            NotificationLevel::Mayday => "🚨",
        }
    }
}

impl fmt::Display for NotificationLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A notification the daemon or CLI wants to surface.
#[derive(Debug, Clone)]
pub struct Notification {
    pub level: NotificationLevel,
    pub title: String,
    pub body: String,
}

impl Notification {
    pub fn new(
        level: NotificationLevel,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            level,
            title: title.into(),
            body: body.into(),
        }
    }
}

/// Trait for anything that can emit a user-facing notification.
pub trait Notifier: Send + Sync {
    fn notify(&self, notification: Notification) -> anyhow::Result<()>;
}

/// Remove control characters (including the ANSI `ESC` introducer) from text
/// destined for a terminal, keeping newlines and tabs. Prevents a crafted
/// `task`/`cwd` from injecting terminal escape sequences.
fn strip_control_chars(s: &str) -> String {
    s.chars()
        .filter(|&c| c == '\n' || c == '\t' || !c.is_control())
        .collect()
}

/// Print the notification to stderr. Always available.
pub struct TerminalNotifier;

impl Notifier for TerminalNotifier {
    fn notify(&self, n: Notification) -> anyhow::Result<()> {
        let title = strip_control_chars(&n.title);
        let body = strip_control_chars(&n.body);
        eprintln!("{} [{}] {} — {}", n.level.icon(), n.level, title, body);
        Ok(())
    }
}

/// Test notifier that records all notifications for inspection.
#[derive(Debug, Default)]
pub struct TestNotifier {
    pub notifications: std::sync::Mutex<Vec<Notification>>,
}

impl TestNotifier {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.notifications.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn drain(&self) -> Vec<Notification> {
        self.notifications.lock().unwrap().drain(..).collect()
    }

    pub fn contains(&self, predicate: impl Fn(&Notification) -> bool) -> bool {
        self.notifications.lock().unwrap().iter().any(predicate)
    }
}

impl Notifier for TestNotifier {
    fn notify(&self, n: Notification) -> anyhow::Result<()> {
        self.notifications.lock().unwrap().push(n);
        Ok(())
    }
}

/// Escape text for display in a freedesktop desktop notification.
///
/// The freedesktop notification spec permits a small HTML markup subset
/// (including `<a href>` hyperlinks) in the summary and body, and `notify-rust`
/// does not escape for us. Neutralise the markup-significant characters so a
/// `task` or `cwd` string cannot render as markup or a clickable link.
#[cfg(feature = "notify")]
fn escape_desktop_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Desktop notification backend via D-Bus / macOS Notification Center.
#[cfg(feature = "notify")]
pub struct DesktopNotifier {
    app_name: String,
}

#[cfg(feature = "notify")]
impl DesktopNotifier {
    pub fn new(app_name: impl Into<String>) -> Self {
        Self {
            app_name: app_name.into(),
        }
    }
}

#[cfg(feature = "notify")]
impl Notifier for DesktopNotifier {
    fn notify(&self, n: Notification) -> anyhow::Result<()> {
        let mut builder = notify_rust::Notification::new();
        builder
            .appname(&self.app_name)
            .summary(&format!(
                "{} {}",
                n.level.icon(),
                escape_desktop_markup(&n.title)
            ))
            .body(&escape_desktop_markup(&n.body));

        match n.level {
            NotificationLevel::Mayday => {
                builder.urgency(notify_rust::Urgency::Critical);
            }
            NotificationLevel::NoreasterWarning => {
                builder.urgency(notify_rust::Urgency::Normal);
            }
            _ => {
                builder.urgency(notify_rust::Urgency::Low);
            }
        }

        builder.show()?;
        Ok(())
    }
}

/// Pick the best notifier available on this build.
pub fn default_notifier() -> Arc<dyn Notifier> {
    #[cfg(feature = "notify")]
    {
        Arc::new(DesktopNotifier::new("Scotia"))
    }
    #[cfg(not(feature = "notify"))]
    {
        Arc::new(TerminalNotifier)
    }
}

/// Build a Nova Scotia themed notification for a run lifecycle event.
pub fn run_started(agent: AgentKind, cwd: &Path, task: Option<&str>) -> Notification {
    let harbour = cwd
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("home port");
    let body = if let Some(t) = task {
        format!("Agent {} casting off for task: {}", agent.as_str(), t)
    } else {
        format!("Agent {} casting off from {}", agent.as_str(), harbour)
    };
    Notification::new(
        NotificationLevel::LightFlurries,
        format!("Casting off — {}", agent.as_str()),
        body,
    )
}

pub fn run_finished(
    agent: AgentKind,
    actions: usize,
    models: usize,
    errors: usize,
    retries: usize,
) -> Notification {
    if errors == 0 && retries == 0 {
        Notification::new(
            NotificationLevel::HarbourClear,
            format!("Returned to port — {}", agent.as_str()),
            format!("{} actions logged, {} models routed.", actions, models),
        )
    } else {
        Notification::new(
            NotificationLevel::NoreasterWarning,
            format!("Nor'easter warning — {}", agent.as_str()),
            format!(
                "Nor'easter off Cape Breton: {} errors, {} retries across {} actions.",
                errors, retries, actions
            ),
        )
    }
}

pub fn run_crashed(agent: AgentKind, exit_code: Option<i32>) -> Notification {
    let detail = exit_code
        .map(|c| format!("exit code {}", c))
        .unwrap_or_else(|| "killed by signal".to_string());
    Notification::new(
        NotificationLevel::Mayday,
        format!("Mayday — {}", agent.as_str()),
        format!("Went down in heavy seas ({}).", detail),
    )
}

pub fn run_still_active(agent: AgentKind, duration: Duration) -> Notification {
    let mins = duration.as_secs() / 60;
    Notification::new(
        NotificationLevel::IceField,
        format!("In the ice field — {}", agent.as_str()),
        format!("Still underway after {} minutes.", mins),
    )
}

pub fn daemon_started() -> Notification {
    Notification::new(
        NotificationLevel::LightFlurries,
        "Scotia daemon online",
        "Light flurries at the harbour — ready to cast off.",
    )
}

pub fn shims_installed(count: usize) -> Notification {
    Notification::new(
        NotificationLevel::HarbourClear,
        "Shims installed",
        format!("{} lighthouses now guiding agent calls.", count),
    )
}

pub fn shims_uninstalled() -> Notification {
    Notification::new(
        NotificationLevel::HarbourClear,
        "Shims removed",
        "Harbour cleared — agents will run unwrapped.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_started_notification_uses_maritime_body() {
        let n = run_started(AgentKind::ClaudeCode, Path::new("/home/sal/project"), None);
        assert!(matches!(n.level, NotificationLevel::LightFlurries));
        assert!(n.body.contains("casting off"));
        assert!(n.body.contains("project"));
    }

    #[test]
    fn run_finished_success_is_harbour_clear() {
        let n = run_finished(AgentKind::KimiCode, 12, 3, 0, 0);
        assert!(matches!(n.level, NotificationLevel::HarbourClear));
        assert!(n.body.contains("12 actions"));
    }

    #[test]
    fn run_finished_with_errors_is_noreaster() {
        let n = run_finished(AgentKind::Codex, 12, 3, 2, 1);
        assert!(matches!(n.level, NotificationLevel::NoreasterWarning));
        assert!(n.body.contains("Cape Breton"));
    }

    // ANSI/terminal control characters in a `task`/`cwd` must not reach the
    // terminal, but newlines and tabs (legitimate formatting) are preserved.
    #[test]
    fn strip_control_chars_removes_ansi_but_keeps_whitespace() {
        let input = "a\x1b[31mred\x1b[0m\n\tz";
        let out = strip_control_chars(input);
        assert_eq!(out, "a[31mred[0m\n\tz");
        assert!(
            !out.chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t'),
            "only newline/tab control chars may survive"
        );
    }

    // Desktop notifications permit a small HTML subset; markup-significant
    // characters must be escaped so a crafted string cannot render as a link.
    #[cfg(feature = "notify")]
    #[test]
    fn escape_desktop_markup_neutralises_html() {
        assert_eq!(
            escape_desktop_markup(r#"<a href="x">&'"#),
            "&lt;a href=&quot;x&quot;&gt;&amp;'"
        );
    }
}
