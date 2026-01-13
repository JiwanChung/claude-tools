use crate::app::App;
use crate::detector::Status;
use crate::pricing::{format_cost, format_tokens};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // Pane list
            Constraint::Length(3), // Footer/help
        ])
        .split(frame.area());

    render_header(frame, app, chunks[0]);
    if app.show_stats {
        render_stats(frame, app, chunks[1]);
    } else {
        render_pane_list(frame, app, chunks[1]);
    }
    render_footer(frame, chunks[2]);
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let summary = app.summary();

    let title = vec![
        Span::styled(
            " Claude Monitor ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("{} sessions", summary.total),
            Style::default().fg(Color::White),
        ),
    ];

    let mut status_line = vec![
        Span::raw(" "),
        status_badge(">_", summary.waiting, Color::Green, "waiting"),
        Span::raw("  "),
        status_badge("◐", summary.working, Color::Yellow, "working"),
    ];

    if summary.permission > 0 {
        status_line.push(Span::raw("  "));
        status_line.push(status_badge("⚠", summary.permission, Color::Red, "permission"));
    }

    let header = Paragraph::new(vec![Line::from(title), Line::from(status_line)]).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    frame.render_widget(header, area);
}

fn status_badge(icon: &str, count: usize, color: Color, label: &str) -> Span<'static> {
    Span::styled(
        format!("{} {} {}", icon, count, label),
        Style::default().fg(color),
    )
}

fn render_pane_list(frame: &mut Frame, app: &App, area: Rect) {
    let panes = app.visible_panes();

    if panes.is_empty() {
        let message = if app.show_all_panes {
            "No tmux panes found. Is tmux running?"
        } else {
            "No Claude Code sessions found. Press 'a' to show all panes."
        };

        let empty = Paragraph::new(message)
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Panes ")
                    .border_style(Style::default().fg(Color::DarkGray)),
            );

        frame.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = if app.group_by_session {
        render_grouped_items(app, &panes, app.selected_index)
    } else if app.compact_mode {
        render_compact_items(&panes, app.selected_index)
    } else {
        render_full_items(&panes, app.selected_index)
    };

    let title = if app.show_all_panes {
        " All Panes "
    } else {
        " Claude Code Sessions "
    };

    let filter_suffix = match app.status_filter {
        Some(Status::Working) => " [working only]",
        Some(Status::WaitingForInput) => " [waiting only]",
        _ => "",
    };

    let compact_suffix = if app.compact_mode { " [compact]" } else { "" };
    let group_suffix = if app.group_by_session { " [grouped]" } else { "" };
    let title_suffix = format!("{}{}{}", filter_suffix, compact_suffix, group_suffix);

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("{}{}", title, title_suffix))
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    frame.render_widget(list, area);
}

fn render_grouped_items(app: &App, panes: &[&crate::app::PaneState], selected_index: usize) -> Vec<ListItem<'static>> {
    let mut items = Vec::new();
    let mut current_session: Option<String> = None;

    for (i, pane_state) in panes.iter().enumerate() {
        let session = &pane_state.pane.session_name;

        // Add session header if this is a new session
        if current_session.as_ref() != Some(session) {
            current_session = Some(session.clone());
            let is_collapsed = app.collapsed_sessions.contains(session);

            // Count panes in this session
            let session_panes: Vec<_> = panes.iter()
                .filter(|p| &p.pane.session_name == session)
                .collect();
            let working = session_panes.iter().filter(|p| p.status.status == Status::Working).count();
            let waiting = session_panes.iter().filter(|p| p.status.status == Status::WaitingForInput).count();
            let permission = session_panes.iter().filter(|p| p.status.status == Status::PermissionRequired).count();

            let collapse_icon = if is_collapsed { "▶" } else { "▼" };
            let mut header_spans = vec![
                Span::styled(
                    format!(" {} ", collapse_icon),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    session.clone(),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  ({} panes", session_panes.len()),
                    Style::default().fg(Color::DarkGray),
                ),
            ];
            if permission > 0 {
                header_spans.push(Span::styled(format!(", {} ⚠", permission), Style::default().fg(Color::Red)));
            }
            if working > 0 {
                header_spans.push(Span::styled(format!(", {} working", working), Style::default().fg(Color::Yellow)));
            }
            if waiting > 0 {
                header_spans.push(Span::styled(format!(", {} waiting", waiting), Style::default().fg(Color::Green)));
            }
            header_spans.push(Span::styled(")", Style::default().fg(Color::DarkGray)));

            let header = Line::from(header_spans);

            items.push(ListItem::new(header));

            // Skip panes if collapsed
            if is_collapsed {
                continue;
            }
        }

        // Skip if session is collapsed
        if app.collapsed_sessions.contains(session) {
            continue;
        }

        // Render the pane (compact style, indented)
        let status = pane_state.status.status;
        let is_selected = i == selected_index;

        let (status_color, status_icon) = match status {
            Status::WaitingForInput => (Color::Green, ">_"),
            Status::PermissionRequired => (Color::Red, "⚠ "),
            Status::Working => (Color::Yellow, "◐ "),
            Status::NotClaudeCode => (Color::DarkGray, "--"),
        };

        let (_, folder_name) = split_path(&pane_state.pane.current_path);

        let mut spans = vec![
            Span::raw("   "), // Indent under session
            Span::styled(
                format!("{} ", status_icon),
                Style::default().fg(status_color),
            ),
            Span::styled(
                format!("{}.{}", pane_state.pane.window_index, pane_state.pane.pane_index),
                Style::default().add_modifier(if is_selected {
                    Modifier::BOLD | Modifier::UNDERLINED
                } else {
                    Modifier::BOLD
                }),
            ),
            Span::raw(" "),
            Span::styled(folder_name, Style::default().fg(Color::Cyan)),
            Span::styled(
                format!(" {}", pane_state.status_duration_str()),
                Style::default().fg(Color::DarkGray),
            ),
        ];

        // Add session cost if available
        if let Some(ref cost) = pane_state.status.session_cost {
            spans.push(Span::styled(
                format!(" {}", format_cost(cost.cost_usd)),
                Style::default().fg(Color::Magenta),
            ));
        } else if let Some(ref tokens) = pane_state.status.tokens {
            spans.push(Span::styled(
                format!(" ({})", tokens),
                Style::default().fg(Color::Magenta),
            ));
        }

        let style = if is_selected {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };

        items.push(ListItem::new(Line::from(spans)).style(style));
    }

    items
}

fn render_compact_items(panes: &[&crate::app::PaneState], selected_index: usize) -> Vec<ListItem<'static>> {
    panes
        .iter()
        .enumerate()
        .map(|(i, pane_state)| {
            let status = pane_state.status.status;
            let is_selected = i == selected_index;

            let (status_color, status_icon) = match status {
                Status::WaitingForInput => (Color::Green, ">_"),
                Status::PermissionRequired => (Color::Red, "⚠ "),
                Status::Working => (Color::Yellow, "◐ "),
                Status::NotClaudeCode => (Color::DarkGray, "--"),
            };

            // Get just the folder name for compact view
            let (_, folder_name) = split_path(&pane_state.pane.current_path);

            let mut spans = vec![
                Span::styled(
                    format!(" {} ", status_icon),
                    Style::default().fg(status_color),
                ),
                Span::styled(
                    pane_state.pane.display_name(),
                    Style::default().add_modifier(if is_selected {
                        Modifier::BOLD | Modifier::UNDERLINED
                    } else {
                        Modifier::BOLD
                    }),
                ),
                Span::raw(" "),
                Span::styled(
                    folder_name,
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!(" {}", pane_state.status_duration_str()),
                    Style::default().fg(Color::DarkGray),
                ),
            ];

            // Add session cost if available, otherwise fallback to tokens
            if let Some(ref cost) = pane_state.status.session_cost {
                spans.push(Span::styled(
                    format!(" {} ({})", format_cost(cost.cost_usd), format_tokens(cost.usage.total())),
                    Style::default().fg(Color::Magenta),
                ));
            } else if let Some(ref tokens) = pane_state.status.tokens {
                spans.push(Span::styled(
                    format!(" ({})", tokens),
                    Style::default().fg(Color::Magenta),
                ));
            }

            let style = if is_selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(spans)).style(style)
        })
        .collect()
}

fn render_full_items(panes: &[&crate::app::PaneState], selected_index: usize) -> Vec<ListItem<'static>> {
    panes
        .iter()
        .enumerate()
        .map(|(i, pane_state)| {
            let status = pane_state.status.status;
            let is_selected = i == selected_index;

            let (status_color, status_icon) = match status {
                Status::WaitingForInput => (Color::Green, ">_"),
                Status::PermissionRequired => (Color::Red, "⚠ "),
                Status::Working => (Color::Yellow, "◐ "),
                Status::NotClaudeCode => (Color::DarkGray, "--"),
            };

            // Shorten path for display and split into parent + folder name
            let (parent_path, folder_name) = split_path(&pane_state.pane.current_path);

            let line1 = Line::from(vec![
                Span::styled(
                    format!(" {} ", status_icon),
                    Style::default().fg(status_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    pane_state.pane.display_name(),
                    Style::default().add_modifier(if is_selected {
                        Modifier::BOLD | Modifier::UNDERLINED
                    } else {
                        Modifier::BOLD
                    }),
                ),
                Span::raw("  "),
                Span::styled(parent_path, Style::default().fg(Color::DarkGray)),
                Span::styled(
                    folder_name,
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
            ]);

            let mut line2_spans = vec![
                Span::raw("     "),
                Span::styled(
                    status.label(),
                    Style::default().fg(status_color),
                ),
                Span::styled(
                    format!(" {}", pane_state.status_duration_str()),
                    Style::default().fg(Color::DarkGray),
                ),
            ];

            // Add session cost and token usage if available
            if let Some(ref cost) = pane_state.status.session_cost {
                line2_spans.push(Span::styled(
                    format!("  {} ({})", format_cost(cost.cost_usd), format_tokens(cost.usage.total())),
                    Style::default().fg(Color::Magenta),
                ));
            } else if let Some(ref tokens) = pane_state.status.tokens {
                // Fallback to screen-scraped tokens
                line2_spans.push(Span::styled(
                    format!(" ({})", tokens),
                    Style::default().fg(Color::Magenta),
                ));
            }

            let line2 = Line::from(line2_spans);

            let mut lines = vec![line1, line2];

            // Add pane task (from tmux title) if present
            if let Some(ref task) = pane_state.status.pane_task {
                let task_line = Line::from(vec![
                    Span::raw("     "),
                    Span::styled(
                        task.chars().take(70).collect::<String>(),
                        Style::default().fg(Color::Cyan),
                    ),
                ]);
                lines.push(task_line);
            }

            // Add user prompt on separate line if present
            if let Some(ref prompt) = pane_state.status.last_user_prompt {
                let truncated: String = prompt.chars().take(80).collect();
                let prompt_line = Line::from(vec![
                    Span::raw("     "),
                    Span::styled("> ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        truncated,
                        Style::default().fg(Color::White),
                    ),
                ]);
                lines.push(prompt_line);
            }

            // Add tool info if working
            if status == Status::Working {
                if let (Some(ref tool), Some(ref detail)) = (&pane_state.status.current_tool, &pane_state.status.tool_detail) {
                    let tool_line = Line::from(vec![
                        Span::raw("     "),
                        Span::styled(format!("{}: ", tool), Style::default().fg(Color::Magenta)),
                        Span::styled(
                            detail.chars().take(60).collect::<String>(),
                            Style::default().fg(Color::White),
                        ),
                    ]);
                    lines.push(tool_line);
                }
            }

            // Add detail on separate line(s) if present (and no user prompt shown)
            if pane_state.status.last_user_prompt.is_none() {
                if let Some(ref detail) = pane_state.status.detail {
                    for detail_part in detail.lines() {
                        let detail_line = Line::from(vec![
                            Span::raw("       "),
                            Span::styled(
                                detail_part.to_string(),
                                Style::default().fg(Color::White),
                            ),
                        ]);
                        lines.push(detail_line);
                    }
                }
            }

            let style = if is_selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            ListItem::new(lines).style(style)
        })
        .collect()
}

fn render_stats(frame: &mut Frame, app: &App, area: Rect) {
    use crate::app::format_duration;

    let stats = app.aggregated_stats();
    let panes = app.visible_panes();

    // Calculate total cost and tokens across all panes
    let mut total_cost: f64 = 0.0;
    let mut total_tokens: u64 = 0;
    for pane in &panes {
        if let Some(ref cost) = pane.status.session_cost {
            total_cost += cost.cost_usd;
            total_tokens += cost.usage.total();
        }
    }

    let mut lines = vec![
        Line::from(vec![
            Span::styled(" Aggregated Stats ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  Panes monitored: "),
            Span::styled(format!("{}", stats.pane_count), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::raw("  State changes:   "),
            Span::styled(format!("{}", stats.total_state_changes), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Token Usage & Cost:", Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::raw("    Total tokens: "),
            Span::styled(
                format_tokens(total_tokens),
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw("    Total cost:   "),
            Span::styled(
                format_cost(total_cost),
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Time in states:", Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::raw("    Working:    "),
            Span::styled(
                format_duration(std::time::Duration::from_secs(stats.total_working_secs)),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(vec![
            Span::raw("    Waiting:    "),
            Span::styled(
                format_duration(std::time::Duration::from_secs(stats.total_waiting_secs)),
                Style::default().fg(Color::Green),
            ),
        ]),
        Line::from(vec![
            Span::raw("    Permission: "),
            Span::styled(
                format_duration(std::time::Duration::from_secs(stats.total_permission_secs)),
                Style::default().fg(Color::Red),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  Efficiency: "),
            Span::styled(
                format!("{:.1}%", stats.efficiency_percent()),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" (working / total)", Style::default().fg(Color::DarkGray)),
        ]),
    ];

    // Add per-pane stats
    if !panes.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  Per-pane breakdown:", Style::default().add_modifier(Modifier::BOLD)),
        ]));

        for pane in panes.iter().take(10) {
            let (_, folder) = split_path(&pane.pane.current_path);

            let cost_str = if let Some(ref cost) = pane.status.session_cost {
                format!(" {} ({})", format_cost(cost.cost_usd), format_tokens(cost.usage.total()))
            } else {
                String::new()
            };

            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(
                    format!("{:<12}", folder.chars().take(12).collect::<String>()),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    cost_str,
                    Style::default().fg(Color::Magenta),
                ),
            ]));
        }
    }

    let stats_widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Stats (press 's' to close) ")
            .border_style(Style::default().fg(Color::Cyan)),
    );

    frame.render_widget(stats_widget, area);
}

fn render_footer(frame: &mut Frame, area: Rect) {
    let help = Line::from(vec![
        Span::styled(" q ", Style::default().fg(Color::Yellow)),
        Span::raw("quit  "),
        Span::styled(" ↑↓ ", Style::default().fg(Color::Yellow)),
        Span::raw("nav  "),
        Span::styled(" ⏎ ", Style::default().fg(Color::Yellow)),
        Span::raw("jump  "),
        Span::styled(" y ", Style::default().fg(Color::Yellow)),
        Span::raw("yes  "),
        Span::styled(" s ", Style::default().fg(Color::Yellow)),
        Span::raw("stats  "),
        Span::styled(" g ", Style::default().fg(Color::Yellow)),
        Span::raw("group  "),
        Span::styled(" w/i ", Style::default().fg(Color::Yellow)),
        Span::raw("filter"),
    ]);

    let footer = Paragraph::new(help).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    frame.render_widget(footer, area);
}

/// Split path into (parent, folder_name) with ~ substitution
fn split_path(path: &str) -> (String, String) {
    // Replace home directory with ~
    let home = std::env::var("HOME").unwrap_or_default();
    let normalized = if !home.is_empty() && path.starts_with(&home) {
        format!("~{}", &path[home.len()..])
    } else {
        path.to_string()
    };

    // Split into parent and folder name
    if let Some(pos) = normalized.rfind('/') {
        let parent = &normalized[..=pos]; // Include trailing /
        let folder = &normalized[pos + 1..];
        (parent.to_string(), folder.to_string())
    } else {
        (String::new(), normalized)
    }
}
