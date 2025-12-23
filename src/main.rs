mod app;
mod budget;
mod detector;
mod notify;
mod resume;
mod sync;
mod tmux;
mod ui;

use anyhow::Result;
use app::App;
use clap::{Parser, Subcommand};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "claude-tools")]
#[command(about = "CLI tools for Claude Code")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Monitor Claude Code sessions across tmux panes
    Monitor {
        /// Refresh interval in seconds
        #[arg(short, long, default_value = "2")]
        interval: u64,

        /// Number of lines to capture from each pane
        #[arg(short, long, default_value = "100")]
        lines: usize,

        /// Show all panes, not just Claude Code sessions
        #[arg(short, long)]
        all: bool,

        /// Compact mode (single line per pane)
        #[arg(short, long)]
        compact: bool,

        /// Enable desktop notifications on state change
        #[arg(short, long)]
        notify: bool,

        /// Auto-jump to pane when it becomes ready
        #[arg(short, long)]
        jump: bool,
    },

    /// List and restore previous Claude Code sessions
    Resume {
        #[command(subcommand)]
        action: resume::ResumeAction,
    },

    /// Sync CLAUDE.md files across projects
    Sync {
        #[command(subcommand)]
        action: sync::SyncAction,
    },

    /// Track and manage token budgets
    Budget {
        #[command(subcommand)]
        action: budget::BudgetAction,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Monitor {
            interval,
            lines,
            all,
            compact,
            notify,
            jump,
        } => run_monitor(interval, lines, all, compact, notify, jump),

        Commands::Resume { action } => resume::run(action),
        Commands::Sync { action } => sync::run(action),
        Commands::Budget { action } => budget::run(action),
    }
}

fn run_monitor(
    interval: u64,
    lines: usize,
    all: bool,
    compact: bool,
    notify_enabled: bool,
    jump_enabled: bool,
) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run app
    let result = run_monitor_app(&mut terminal, interval, lines, all, compact, notify_enabled, jump_enabled);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn run_monitor_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    interval: u64,
    lines: usize,
    all: bool,
    compact: bool,
    notify_enabled: bool,
    jump_enabled: bool,
) -> Result<()> {
    let mut app = App::new(lines, all, compact);
    let refresh_interval = Duration::from_secs(interval);

    // Initial refresh
    let _ = app.refresh()?;

    loop {
        // Render
        terminal.draw(|frame| ui::render(frame, &app))?;

        // Poll for events with timeout (for auto-refresh)
        if event::poll(refresh_interval)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            return Ok(());
                        }
                        KeyCode::Char('r') => {
                            for notif in app.refresh()? {
                                if notify_enabled {
                                    if notif.is_permission {
                                        notify::send_notification(
                                            &format!("⚠️ Permission: {}", notif.folder_name),
                                            &format!("{} needs approval", notif.pane_name),
                                        );
                                    } else {
                                        notify::send_notification(
                                            &format!("Claude ready: {}", notif.folder_name),
                                            &format!("{} is waiting for input", notif.pane_name),
                                        );
                                    }
                                }
                                if jump_enabled {
                                    let _ = tmux::switch_to_pane(
                                        &notif.session_name,
                                        notif.window_index,
                                        notif.pane_index,
                                    );
                                    break;
                                }
                            }
                        }
                        KeyCode::Char('a') => app.toggle_show_all(),
                        KeyCode::Char('c') => app.toggle_compact(),
                        KeyCode::Char('w') => app.toggle_filter_working(),
                        KeyCode::Char('i') => app.toggle_filter_waiting(),
                        KeyCode::Char('g') => app.toggle_grouping(),
                        KeyCode::Char('s') => app.toggle_stats(),
                        KeyCode::Char('e') => {
                            let export = app.export_stats();
                            if let Ok(json) = serde_json::to_string_pretty(&export) {
                                let filename = format!("claude-stats-{}.json", export.timestamp);
                                let _ = std::fs::write(&filename, json);
                            }
                        }
                        KeyCode::Tab => {
                            if let Some(pane_state) = app.selected_pane() {
                                let session = pane_state.pane.session_name.clone();
                                app.toggle_session_collapse(&session);
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => app.select_next(),
                        KeyCode::Up | KeyCode::Char('k') => app.select_previous(),
                        KeyCode::Enter => {
                            if let Some(pane_state) = app.selected_pane() {
                                let pane = &pane_state.pane;
                                let _ = tmux::switch_to_pane(
                                    &pane.session_name,
                                    pane.window_index,
                                    pane.pane_index,
                                );
                            }
                        }
                        KeyCode::Char('y') => {
                            if let Some(pane_state) = app.selected_pane() {
                                let _ = tmux::send_keys(&pane_state.pane.id, "y");
                                let _ = tmux::send_keys(&pane_state.pane.id, "Enter");
                            }
                        }
                        _ => {}
                    }
                }
            }
        } else {
            // Timeout expired, refresh data
            for notif in app.refresh()? {
                if notify_enabled {
                    if notif.is_permission {
                        notify::send_notification(
                            &format!("⚠️ Permission: {}", notif.folder_name),
                            &format!("{} needs approval", notif.pane_name),
                        );
                    } else {
                        notify::send_notification(
                            &format!("Claude ready: {}", notif.folder_name),
                            &format!("{} is waiting for input", notif.pane_name),
                        );
                    }
                }
                if jump_enabled {
                    let _ = tmux::switch_to_pane(
                        &notif.session_name,
                        notif.window_index,
                        notif.pane_index,
                    );
                    break;
                }
            }
        }
    }
}
