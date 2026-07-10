use crate::storage::{StorageConfig, store_run};
use crate::wrapper::{WrapperConfig, run_and_capture};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use std::io::{self, IsTerminal};

mod detect;
pub use detect::{Harness, detect_harnesses, detect_harnesses_with_path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    HarnessList,
    TaskInput,
}

struct App {
    harnesses: Vec<Harness>,
    state: ListState,
    task: String,
    focus: Focus,
    storage: StorageConfig,
    message: Option<String>,
    ran_harness: bool,
}

impl App {
    fn new(storage: StorageConfig) -> Self {
        let harnesses = detect_harnesses();
        let mut state = ListState::default();
        if !harnesses.is_empty() {
            state.select(Some(0));
        }
        Self {
            harnesses,
            state,
            task: String::new(),
            focus: Focus::HarnessList,
            storage,
            message: None,
            ran_harness: false,
        }
    }

    fn selected_harness(&self) -> Option<&Harness> {
        self.state.selected().and_then(|i| self.harnesses.get(i))
    }

    fn next(&mut self) {
        if self.harnesses.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => (i + 1) % self.harnesses.len(),
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn previous(&mut self) {
        if self.harnesses.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.harnesses.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }
}

/// Open the interactive harness-selection TUI, or fall back to a text menu
/// when stdin is not a terminal.
pub async fn run_tui(storage: StorageConfig) -> Result<()> {
    let app = App::new(storage);

    if !io::stdin().is_terminal() {
        return run_text_fallback(app).await;
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = app;
    let result = run_app(&mut terminal, &mut app).await;

    if app.ran_harness {
        // run_harness already restored the terminal and left the alternate screen.
        return result;
    }

    disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run_text_fallback(app: App) -> Result<()> {
    eprintln!("Stdin is not a terminal, so the interactive TUI cannot open.\n");

    if app.harnesses.is_empty() {
        eprintln!("No agent harnesses were detected on PATH.");
        eprintln!("Install an agent (e.g. Claude Code, Kimi, Codex) and try again,");
        eprintln!("or use: scotia run --agent <kind> -- <command>");
    } else {
        eprintln!("Detected harnesses:");
        for (i, h) in app.harnesses.iter().enumerate() {
            eprintln!(
                "  {}. {} ({}) — {} {}",
                i + 1,
                h.display_name,
                h.agent.as_str(),
                h.binary.display(),
                h.args.join(" ")
            );
        }
        eprintln!();
        if let Some(first) = app.harnesses.first() {
            eprintln!(
                "Run one with: scotia run --agent {} -- {} {}",
                first.agent.as_str(),
                first.binary.display(),
                first.args.join(" ")
            );
        }
        eprintln!("Or run in a real terminal to use the interactive selector.");
    }

    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match app.focus {
                Focus::HarnessList => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Down | KeyCode::Char('j') => app.next(),
                    KeyCode::Up | KeyCode::Char('k') => app.previous(),
                    KeyCode::Tab | KeyCode::Char('t') => app.focus = Focus::TaskInput,
                    KeyCode::Enter => {
                        if let Some(harness) = app.selected_harness().cloned() {
                            run_harness(terminal, app, &harness).await?;
                        } else {
                            app.message = Some("No harness selected".to_string());
                        }
                    }
                    _ => {}
                },
                Focus::TaskInput => match key.code {
                    KeyCode::Esc => app.focus = Focus::HarnessList,
                    KeyCode::Enter => {
                        app.focus = Focus::HarnessList;
                        if let Some(harness) = app.selected_harness().cloned() {
                            run_harness(terminal, app, &harness).await?;
                        }
                    }
                    KeyCode::Char(c) => app.task.push(c),
                    KeyCode::Backspace => {
                        app.task.pop();
                    }
                    _ => {}
                },
            }
        }
    }
}

async fn run_harness(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    harness: &Harness,
) -> Result<()> {
    terminal.draw(|f| {
        let area = f.area();
        let block = Block::default()
            .title("Running harness...")
            .borders(Borders::ALL);
        let paragraph = Paragraph::new(format!(
            "Starting {}\nTask: {}\n\nPress Ctrl+C in the agent to abort.",
            harness.display_name,
            if app.task.is_empty() {
                "(none)"
            } else {
                &app.task
            }
        ))
        .block(block);
        f.render_widget(Clear, area);
        f.render_widget(paragraph, area);
    })?;

    // Leave the TUI permanently so the agent's own TUI/stdio works natively.
    // We do not re-enter the alternate screen; the program exits after the run.
    disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    let task = if app.task.is_empty() {
        None
    } else {
        Some(app.task.clone())
    };

    let config = WrapperConfig {
        agent: harness.agent,
        task,
        program: harness.binary.display().to_string(),
        args: harness.args.clone(),
        working_dir: None,
        run_id: None,
    };

    app.ran_harness = true;

    match run_and_capture(config).await {
        Ok(run) => {
            let stored = store_run(&app.storage, run).await;
            match stored {
                Ok(s) => {
                    println!("Scotia captured run {}", s.run_id);
                    println!("  JSON:    {}", s.json_path.display());
                    println!("  Summary: {}", s.summary_path.display());
                    println!("  Graph:   {}", s.dot_path.display());
                }
                Err(e) => {
                    eprintln!("Run captured but storage failed: {}", e);
                }
            }
        }
        Err(e) => {
            eprintln!("Run failed: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

fn ui(f: &mut ratatui::Frame, app: &App) {
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
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
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
