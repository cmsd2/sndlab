//! Top-level TUI state. Holds the editor, the log pane, and assorted
//! status fields. Pure state + input dispatch; rendering lives in
//! `ui::render`.

use ratatui::widgets::{Block, Borders};
use tui_textarea::{Input, Key, TextArea};

use crate::log::{LogKind, LogPane};

pub struct App<'a> {
    pub editor: TextArea<'a>,
    pub log: LogPane,
    pub should_quit: bool,
    /// MCP endpoint string for the status bar. Empty until the MCP
    /// server is wired up.
    pub mcp_endpoint: String,
    /// Filename being edited. Cosmetic for now — the editor doesn't
    /// know about disk yet.
    pub filename: String,
}

impl Default for App<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl App<'_> {
    pub fn new() -> Self {
        let mut editor = TextArea::default();
        editor.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title(" patches.rhai "),
        );
        let mut log = LogPane::default();
        log.info(
            "sndlab ready — Ctrl+R to evaluate, Ctrl+Q to quit (no audio backend yet)",
        );
        Self {
            editor,
            log,
            should_quit: false,
            mcp_endpoint: String::new(),
            filename: "patches.rhai".into(),
        }
    }

    /// Handle one input event. Returns true if the event was consumed
    /// by a command (vs forwarded to the editor).
    pub fn on_input(&mut self, input: Input) -> bool {
        match input {
            // Quit. Ctrl+Q is the explicit hotkey; Ctrl+C would normally
            // also quit but Ctrl+C is the universal "cancel" so we
            // reserve it for a future "cancel current dialog" path and
            // keep Ctrl+Q as the unambiguous quit.
            Input {
                key: Key::Char('q'),
                ctrl: true,
                ..
            } => {
                self.should_quit = true;
                true
            }
            // Evaluate (placeholder — the engine wires in next task).
            Input {
                key: Key::Char('r'),
                ctrl: true,
                ..
            } => {
                self.log.push(
                    LogKind::Info,
                    format!(
                        "eval requested ({} lines, {} bytes) — engine not yet wired",
                        self.editor.lines().len(),
                        self.editor.lines().iter().map(|s| s.len() + 1).sum::<usize>()
                    ),
                );
                true
            }
            // Save (placeholder).
            Input {
                key: Key::Char('s'),
                ctrl: true,
                ..
            } => {
                self.log
                    .push(LogKind::Info, "save requested — project layer not yet wired");
                true
            }
            // Everything else goes to the editor.
            other => {
                self.editor.input(other);
                false
            }
        }
    }
}
