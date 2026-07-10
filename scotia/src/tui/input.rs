//! TUI event loop and entry point: terminal setup, key handling, and running a
//! selected harness (handing the terminal over to the agent's own stdio).

use super::app::{App, Focus};
use super::detect::Harness;
use super::render::ui;
use crate::storage::{StorageConfig, store_run};
use crate::wrapper::{WrapperConfig, run_and_capture};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use std::io::{self, IsTerminal};

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
