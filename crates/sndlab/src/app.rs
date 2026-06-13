//! Top-level TUI state. Holds the editor, the log pane, the engine,
//! and assorted status fields. Pure state + input dispatch; rendering
//! lives in `ui::render`.

use ratatui::widgets::{Block, Borders};
use sndlab_core::Engine;
use tui_textarea::{Input, Key, TextArea};

use crate::log::LogPane;
use crate::syntax::Highlighter;

pub struct App<'a> {
    pub editor: TextArea<'a>,
    pub log: LogPane,
    pub engine: Engine,
    pub highlighter: Highlighter,
    pub should_quit: bool,
    /// MCP endpoint string for the status bar. Empty until the MCP
    /// server is wired up.
    pub mcp_endpoint: String,
    /// Filename being edited. Cosmetic for now — the editor doesn't
    /// know about disk yet.
    pub filename: String,
}

impl App<'_> {
    pub fn new() -> Self {
        let mut editor = TextArea::default();
        editor.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title(" patches.rhai "),
        );
        // Seed the editor with a small example so the first Ctrl+R
        // does something audible without the user having to type
        // anything first.
        for line in [
            r#"patch("ping", "one_shot","#,
            r#"    sine(330.0, 1.5).env(0.008, 1.4).gain(0.32)"#,
            r#"        .with_taps(["#,
            r#"            tap(0.13, 0.55),"#,
            r#"            tap(0.31, 0.38),"#,
            r#"            tap(0.58, 0.26),"#,
            r#"        ]));"#,
        ] {
            editor.insert_str(line);
            editor.insert_newline();
        }

        let mut log = LogPane::default();
        let engine = Engine::new().unwrap_or_else(|e| {
            log.error(format!("engine init failed: {e}"));
            panic!("engine init failed: {e}");
        });
        if engine.has_audio() {
            log.info("sndlab ready — Ctrl+R evaluates and plays the first patch");
        } else {
            log.warn("sndlab ready, but audio backend unavailable — playback will be silent");
        }
        log.info("Ctrl+R eval+play   Ctrl+S save (todo)   Ctrl+Q quit");

        Self {
            editor,
            log,
            engine,
            highlighter: Highlighter::new(),
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
            // Evaluate the buffer and play the first registered patch.
            Input {
                key: Key::Char('r'),
                ctrl: true,
                ..
            } => {
                self.eval_and_play();
                true
            }
            // Save (placeholder — project layer in task 9).
            Input {
                key: Key::Char('s'),
                ctrl: true,
                ..
            } => {
                self.log
                    .info("save requested — project layer not yet wired");
                true
            }
            // Everything else goes to the editor.
            other => {
                self.editor.input(other);
                false
            }
        }
    }

    /// Compile the buffer through Rhai, then auto-play the first
    /// patch. The "first patch" rule is a Phase-1 convenience; once
    /// the project model lands the user gets to pick.
    fn eval_and_play(&mut self) {
        let source = self.editor.lines().join("\n");
        match self.engine.eval(&source) {
            Ok(summary) => {
                self.log.info(format!(
                    "eval ok — {} patch(es): [{}]",
                    summary.patches.len(),
                    summary
                        .patches
                        .iter()
                        .map(|p| p.name.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                for m in &summary.messages {
                    self.log.warn(m.clone());
                }
                if let Some(first) = summary.patches.first() {
                    match self.engine.play(&first.name) {
                        Ok(()) => self
                            .log
                            .audio(format!("playing '{}' ({:.2} s)", first.name, first.duration_s)),
                        Err(e) => self.log.error(format!("play failed: {e}")),
                    }
                } else {
                    self.log.warn("no patches registered — script ran but didn't call patch(...)");
                }
            }
            Err(e) => {
                self.log.error(format!("eval failed: {e}"));
            }
        }
    }
}
