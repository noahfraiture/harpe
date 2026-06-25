use harpe_proto::pb;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Tabs, Wrap,
};

use super::state::{App, FinderMode, RightTab};
use super::text::{blank_as, first_sentences, role_name, truncate, wrap_owned};
use crate::serving_status_name;

pub(super) fn render(frame: &mut ratatui::Frame<'_>, app: &mut App) {
    let area = frame.area();
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(12),
            Constraint::Length(6),
            Constraint::Length(1),
        ])
        .split(area);

    render_header(frame, app, root[0]);
    render_body(frame, app, root[1]);
    render_composer(frame, app, root[2]);
    render_footer(frame, app, root[3]);

    if app.help_open {
        render_help(frame, area);
    }
    if app.finder.is_some() {
        render_finder(frame, app, area);
    }
    if let Some(error) = app.error.as_ref() {
        render_error(frame, error, area);
    }
}

fn render_header(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let game = app
        .game
        .as_ref()
        .map(|game| game.title.as_str())
        .unwrap_or("No game");
    let session = app
        .session
        .as_ref()
        .map(|session| session.title.as_str())
        .unwrap_or("No session");
    let model = app.model.as_deref().unwrap_or("server default");
    let health = app
        .health
        .as_ref()
        .map(|health| {
            format!(
                "{} p:{} f:{}",
                serving_status_name(health.status),
                health.pending_jobs,
                health.failed_jobs
            )
        })
        .unwrap_or_else(|| "health unknown".to_owned());
    let title = format!("Harpe - {game} / {session}");
    let meta = format!(
        "user: {} | model: {model} | {health} | {}",
        app.user_id, app.status
    );

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);
    frame.render_widget(
        Paragraph::new(title).style(Style::default().fg(Color::Cyan)),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(meta)
            .alignment(Alignment::Right)
            .style(Style::default().fg(Color::Gray)),
        chunks[1],
    );
}

fn render_body(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    if area.width >= 116 {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(31),
                Constraint::Min(42),
                Constraint::Length(34),
            ])
            .split(area);
        render_scene(frame, app, chunks[0]);
        render_transcript(frame, app, chunks[1]);
        render_right_panel(frame, app, chunks[2]);
    } else if area.width >= 82 {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(45), Constraint::Length(32)])
            .split(area);
        render_transcript(frame, app, chunks[0]);
        render_right_panel(frame, app, chunks[1]);
    } else {
        render_transcript(frame, app, area);
    }
}

fn render_scene(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        "Location",
        Style::default().fg(Color::Yellow),
    )));
    if let Some(location) = app.locations.first() {
        lines.push(Line::from(location.name.clone()));
        lines.push(Line::from(truncate(&location.description, 120)));
    } else {
        lines.push(Line::from("Unknown"));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Situation",
        Style::default().fg(Color::Yellow),
    )));
    lines.push(Line::from(scene_summary(app)));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Open Threads",
        Style::default().fg(Color::Yellow),
    )));
    for event in app.events.iter().rev().take(4) {
        lines.push(Line::from(format!("- {}", truncate(&event.summary, 54))));
    }
    if app.events.is_empty() {
        lines.push(Line::from("- No major events yet"));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Recent Context",
        Style::default().fg(Color::Yellow),
    )));
    for fact in app.facts.iter().take(3) {
        lines.push(Line::from(format!("- {}", truncate(&fact.content, 54))));
    }
    if app.facts.is_empty() {
        lines.push(Line::from("- No world facts yet"));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Scene"))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn scene_summary(app: &App) -> String {
    app.summary
        .as_ref()
        .map(|summary| first_sentences(&summary.content, 2))
        .filter(|summary| !summary.is_empty())
        .or_else(|| app.session.as_ref().map(|session| session.title.clone()))
        .unwrap_or_else(|| "Select a session to begin.".to_owned())
}

fn render_transcript(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let mut lines = Vec::new();
    if app.messages.is_empty() {
        lines.push(Line::from(Span::styled(
            "No messages yet. Type below and press Enter.",
            Style::default().fg(Color::DarkGray),
        )));
    }
    for message in &app.messages {
        let role = role_name(message.role);
        let role_style = match pb::MessageRole::try_from(message.role).ok() {
            Some(pb::MessageRole::User) => Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            Some(pb::MessageRole::Assistant) => Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
            Some(pb::MessageRole::System) => Style::default().fg(Color::Magenta),
            _ => Style::default().fg(Color::Gray),
        };
        lines.push(Line::from(Span::styled(role, role_style)));
        for line in wrap_owned(&message.content, area.width.saturating_sub(4) as usize) {
            lines.push(Line::from(line));
        }
        lines.push(Line::from(""));
    }

    let mut scrollbar = ScrollbarState::new(lines.len()).position(app.transcript_scroll as usize);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Transcript"))
            .scroll((app.transcript_scroll, 0))
            .wrap(Wrap { trim: false }),
        area,
    );
    frame.render_stateful_widget(
        Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight),
        area,
        &mut scrollbar,
    );
}

fn render_right_panel(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(4)])
        .split(area);
    let titles = RightTab::ALL
        .iter()
        .map(|tab| Line::from(tab.title()))
        .collect::<Vec<_>>();
    let selected = RightTab::ALL
        .iter()
        .position(|tab| *tab == app.right_tab)
        .unwrap_or(0);
    frame.render_widget(
        Tabs::new(titles)
            .select(selected)
            .block(Block::default().borders(Borders::ALL))
            .style(Style::default().fg(Color::Gray))
            .highlight_style(Style::default().fg(Color::Cyan)),
        vertical[0],
    );

    match app.right_tab {
        RightTab::Cast => render_cast(frame, app, vertical[1]),
        RightTab::Lore => render_lore(frame, app, vertical[1]),
        RightTab::Map => render_map(frame, app, vertical[1]),
        RightTab::Events => render_events(frame, app, vertical[1]),
        RightTab::Context => render_context(frame, app, vertical[1]),
    }
}

fn render_cast(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let items = if app.characters.is_empty() {
        vec![ListItem::new("No characters yet")]
    } else {
        app.characters
            .iter()
            .map(|character| {
                ListItem::new(vec![
                    Line::from(Span::styled(
                        character.name.clone(),
                        Style::default().fg(Color::Yellow),
                    )),
                    Line::from(format!(
                        "status: {}",
                        blank_as(&character.status, "unknown")
                    )),
                    Line::from(truncate(&character.description, 90)),
                ])
            })
            .collect()
    };
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title("Cast")),
        area,
    );
}

fn render_lore(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let lines = if app.facts.is_empty() {
        vec![Line::from("No world facts yet")]
    } else {
        app.facts
            .iter()
            .flat_map(|fact| {
                [
                    Line::from(Span::styled(
                        format!("{:.0}% confidence", fact.confidence * 100.0),
                        Style::default().fg(Color::DarkGray),
                    )),
                    Line::from(truncate(&fact.content, 120)),
                    Line::from(""),
                ]
            })
            .collect()
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Lore"))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_map(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let lines = if app.locations.is_empty() {
        vec![Line::from("No locations yet")]
    } else {
        app.locations
            .iter()
            .flat_map(|location| {
                [
                    Line::from(Span::styled(
                        location.name.clone(),
                        Style::default().fg(Color::Yellow),
                    )),
                    Line::from(truncate(&location.description, 120)),
                    Line::from(""),
                ]
            })
            .collect()
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Map"))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_events(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let lines = if app.events.is_empty() {
        vec![Line::from("No events yet")]
    } else {
        app.events
            .iter()
            .rev()
            .flat_map(|event| {
                [
                    Line::from(Span::styled(
                        format!("importance {}", event.importance),
                        Style::default().fg(Color::DarkGray),
                    )),
                    Line::from(truncate(&event.summary, 120)),
                    Line::from(""),
                ]
            })
            .collect()
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Events"))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_context(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let mut lines = Vec::new();
    if !app.search_results.is_empty() {
        lines.push(Line::from(Span::styled(
            "Memory Search",
            Style::default().fg(Color::Yellow),
        )));
        for result in app.search_results.iter().take(6) {
            lines.push(Line::from(truncate(result, 110)));
        }
        lines.push(Line::from(""));
    }

    if let Some(preview) = app.context_preview.as_ref() {
        lines.push(Line::from(Span::styled(
            format!("Context Preview: {} tokens", preview.estimated_tokens),
            Style::default().fg(Color::Yellow),
        )));
        for message in preview.messages.iter().take(8) {
            lines.push(Line::from(Span::styled(
                format!("{} [{}]", role_name(message.role), message.estimated_tokens),
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(truncate(&message.content, 110)));
        }
    } else if app.search_results.is_empty() {
        lines.push(Line::from("Ctrl-P previews injected context."));
        lines.push(Line::from("Ctrl-F searches memory using composer text."));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Context"))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_composer(frame: &mut ratatui::Frame<'_>, app: &mut App, area: Rect) {
    frame.render_widget(&app.composer, area);
}

fn render_footer(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let footer = format!(
        "Enter send | Alt-Enter newline | Ctrl-G game | Ctrl-L session | Ctrl-T panel | Ctrl-P context | Ctrl-F memory | Ctrl-R refresh | ? help | Ctrl-Q quit | {}",
        app.addr
    );
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn render_finder(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let Some(finder) = app.finder.as_ref() else {
        return;
    };
    let popup = centered_rect(70, 70, area);
    frame.render_widget(Clear, popup);
    let rows = match finder.mode {
        FinderMode::Game => finder
            .filtered_games()
            .into_iter()
            .enumerate()
            .map(|(index, game)| finder_item(index, finder.selected, &game.title, &game.id))
            .collect::<Vec<_>>(),
        FinderMode::Session { .. } => finder
            .filtered_sessions()
            .into_iter()
            .enumerate()
            .map(|(index, session)| {
                finder_item(index, finder.selected, &session.title, &session.id)
            })
            .collect::<Vec<_>>(),
    };
    let title = format!("{} - query: {}", finder.title(), finder.query);
    frame.render_widget(
        List::new(rows).block(Block::default().borders(Borders::ALL).title(title)),
        popup,
    );
}

fn finder_item<'a>(index: usize, selected: usize, title: &'a str, id: &'a str) -> ListItem<'a> {
    let style = if index == selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    ListItem::new(Line::from(vec![
        Span::raw(if index == selected { "> " } else { "  " }),
        Span::styled(title.to_owned(), style),
        Span::raw(format!("  {id}")),
    ]))
}

fn render_help(frame: &mut ratatui::Frame<'_>, area: Rect) {
    let popup = centered_rect(62, 54, area);
    frame.render_widget(Clear, popup);
    let lines = vec![
        Line::from("Harpe TUI"),
        Line::from(""),
        Line::from("Enter sends the composer. Alt-Enter or Ctrl-J inserts a newline."),
        Line::from("Ctrl-G opens the game finder; Ctrl-L opens the session finder."),
        Line::from("Ctrl-T cycles Cast/Lore/Map/Events/Context."),
        Line::from("Ctrl-P previews the exact context for the composer text."),
        Line::from("Ctrl-F searches memory using the composer text."),
        Line::from("Ctrl-R refreshes active session data."),
        Line::from("PageUp/PageDown scrolls the transcript."),
        Line::from("Esc closes overlays. Ctrl-Q quits."),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Help"))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn render_error(frame: &mut ratatui::Frame<'_>, error: &str, area: Rect) {
    let popup = centered_rect(60, 25, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("Error", Style::default().fg(Color::Red))),
            Line::from(""),
            Line::from(error.to_owned()),
            Line::from(""),
            Line::from("Press any normal key after closing overlays to continue."),
        ])
        .block(Block::default().borders(Borders::ALL)),
        popup,
    );
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
