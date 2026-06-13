//! TUI rendering. Lays out three vertical zones — editor, status, log —
//! and renders the App into them.

use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
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
    // We render a syntect-highlighted Paragraph in place of
    // tui-textarea's own widget. tui-textarea keeps its role as the
    // *model* — it owns the buffer, cursor position, undo history,
    // search state — but the *view* is ours. This buys per-token
    // colour at the cost of soft-wrapping and selection rendering,
    // both of which are explicit non-goals for now.
    let buffer = app.editor.lines().join("\n");
    let lines = app.highlighter.highlight(&buffer);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", app.filename));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(lines), inner);

    // Place the terminal cursor at the editor model's cursor
    // position, offset by the block's inner origin. The cursor never
    // leaves the visible area because we don't scroll yet; long
    // buffers extend off the bottom of the pane until task 9 wires
    // a real scroll.
    let (cy, cx) = app.editor.cursor();
    let cursor_x = inner.x.saturating_add(cx as u16);
    let cursor_y = inner.y.saturating_add(cy as u16);
    if inner.contains(Position::new(cursor_x, cursor_y)) {
        frame.set_cursor_position(Position::new(cursor_x, cursor_y));
    }
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
