//! TUI rendering: lays out the title, harness list, task input, and footer.

use super::app::{App, Focus};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

pub(crate) fn ui(f: &mut ratatui::Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(f.area());

    let title = Paragraph::new("Scotia — Select an agent harness to observe")
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(title, chunks[0]);

    let items: Vec<ListItem> = app
        .harnesses
        .iter()
        .map(|h| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{} ", h.agent.as_str()),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw(&h.display_name),
                Span::styled(
                    format!("  {}", h.binary.display()),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title("Installed harnesses")
                .borders(Borders::ALL)
                .border_style(if app.focus == Focus::HarnessList {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::Gray)
                }),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    f.render_stateful_widget(list, chunks[1], &mut app.state.clone());

    let task_style = if app.focus == Focus::TaskInput {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::Gray)
    };
    let task_block = Block::default()
        .title("Task description (optional)")
        .borders(Borders::ALL)
        .border_style(task_style);
    let task_text = if app.task.is_empty() {
        "(press Tab to edit)"
    } else {
        &app.task
    };
    let task_para = Paragraph::new(task_text)
        .block(task_block)
        .style(if app.task.is_empty() {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        });
    f.render_widget(task_para, chunks[2]);

    let help = if app.harnesses.is_empty() {
        "No harnesses detected. Install an agent and ensure it is on PATH."
    } else {
        "↑/↓ or j/k: navigate | Tab/t: edit task | Enter: run | q/Esc: quit"
    };
    let mut footer = Line::from(help);
    if let Some(msg) = &app.message {
        footer.spans.push(Span::styled(
            format!("  |  {}", msg),
            Style::default().fg(Color::Green),
        ));
    }
    f.render_widget(Paragraph::new(footer), chunks[3]);
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::event::AgentKind;
    use crate::storage::StorageConfig;
    use crate::tui::detect::Harness;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::widgets::ListState;
    use std::path::PathBuf;

    fn buffer_string(buf: &ratatui::buffer::Buffer) -> String {
        let mut s = String::new();
        for cell in buf.content() {
            s.push_str(cell.symbol());
        }
        s
    }

    /// Golden-ish smoke test: render the main view with a deterministic harness
    /// and assert the key regions are present. Catches accidental layout drift
    /// across ratatui upgrades without pinning every cell.
    #[test]
    fn ui_renders_harness_list_and_footer() {
        let mut state = ListState::default();
        state.select(Some(0));
        let app = App {
            harnesses: vec![Harness::new(
                "claude-code",
                AgentKind::ClaudeCode,
                PathBuf::from("/usr/bin/claude"),
            )],
            state,
            task: String::new(),
            focus: Focus::HarnessList,
            storage: StorageConfig::default(),
            message: None,
            ran_harness: false,
        };

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| ui(f, &app)).unwrap();

        let rendered = buffer_string(terminal.backend().buffer());
        assert!(rendered.contains("Scotia"), "title missing");
        assert!(
            rendered.contains("Installed harnesses"),
            "list block missing"
        );
        assert!(rendered.contains("claude-code"), "harness name missing");
        assert!(rendered.contains("/usr/bin/claude"), "binary path missing");
        assert!(rendered.contains("Enter: run"), "footer help missing");
    }

    #[test]
    fn ui_renders_empty_state_help() {
        let app = App {
            harnesses: vec![],
            state: ListState::default(),
            task: String::new(),
            focus: Focus::HarnessList,
            storage: StorageConfig::default(),
            message: None,
            ran_harness: false,
        };

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| ui(f, &app)).unwrap();

        let rendered = buffer_string(terminal.backend().buffer());
        assert!(
            rendered.contains("No harnesses detected"),
            "empty help missing"
        );
    }
}
