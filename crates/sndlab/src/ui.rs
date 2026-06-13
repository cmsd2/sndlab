//! TUI rendering. Lays out three vertical zones — editor, status, log —
//! and renders the App into them.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::log::LogKind;

pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),    // editor
            Constraint::Length(1), // status
            Constraint::Length(10), // log
        ])
        .split(frame.area());

    render_editor(frame, chunks[0], app);
    render_status(frame, chunks[1], app);
    render_log(frame, chunks[2], app);
}

fn render_editor(frame: &mut Frame, area: Rect, app: &App) {
    // tui-textarea owns its own block; we set the title here to reflect
    // the current filename and (later) modified state.
    let mut editor = app.editor.clone();
    editor.set_block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", app.filename)),
    );
    frame.render_widget(&editor, area);
}

fn render_status(frame: &mut Frame, area: Rect, app: &App) {
    let bytes: usize = app
        .editor
        .lines()
        .iter()
        .map(|s| s.len() + 1)
        .sum();
    let lines = app.editor.lines().len();
    let mcp = if app.mcp_endpoint.is_empty() {
        "MCP: -".to_string()
    } else {
        format!("MCP: {}", app.mcp_endpoint)
    };

    let status = Line::from(vec![
        Span::styled(
            " sndlab ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("  buffer: {} lines, {} bytes  ", lines, bytes)),
        Span::raw(mcp),
        Span::raw("  "),
        Span::styled(
            "Ctrl+R eval   Ctrl+S save   Ctrl+Q quit",
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    frame.render_widget(Paragraph::new(status), area);
}

fn render_log(frame: &mut Frame, area: Rect, app: &App) {
    // Show the most recent lines that fit; newest at the bottom.
    let inner_height = area.height.saturating_sub(2) as usize; // borders
    let entries: Vec<_> = app.log.entries().collect();
    let start = entries.len().saturating_sub(inner_height);
    let visible = &entries[start..];

    let lines: Vec<Line> = visible
        .iter()
        .map(|e| {
            let (tag, colour) = match e.kind {
                LogKind::Info => ("INFO ", Color::Gray),
                LogKind::Warn => ("WARN ", Color::Yellow),
                LogKind::Error => ("ERROR", Color::Red),
                LogKind::Audio => ("AUDIO", Color::Cyan),
            };
            Line::from(vec![
                Span::styled(tag, Style::default().fg(colour).add_modifier(Modifier::BOLD)),
                Span::raw(" "),
                Span::raw(e.line.clone()),
            ])
        })
        .collect();

    let block = Block::default().borders(Borders::ALL).title(" log ");
    frame.render_widget(Paragraph::new(lines).block(block), area);
}
