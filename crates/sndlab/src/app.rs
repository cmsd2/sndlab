//! Top-level eframe app state. Holds the editor buffer, the engine,
//! the log, and the last rendered audio buffer (so the scope can show
//! it). Rendering is in `ui::draw`.

use egui_code_editor::{ColorTheme, Syntax};
use sndlab_core::{Buffer, Engine};

use crate::log::LogPane;

pub struct SndlabApp {
    /// The editor buffer. egui_code_editor mutates this in place.
    pub code: String,
    /// What egui_code_editor uses for syntax highlighting. Rust is
    /// close enough to Rhai visually for now.
    pub syntax: Syntax,
    pub theme: ColorTheme,
    pub log: LogPane,
    pub engine: Engine,
    /// The most recently rendered patch buffer — what the scope shows.
    pub last_buffer: Option<Buffer>,
    /// Endpoint string for the status bar. Empty until the MCP server
    /// is wired up.
    pub mcp_endpoint: String,
    pub filename: String,
}

const SEED_PATCH: &str = r#"// Press F5 to evaluate and play the first patch.

patch("ping", "one_shot",
    sine(330.0, 1.5).env(0.008, 1.4).gain(0.32)
        .with_taps([
            tap(0.13, 0.55),
            tap(0.31, 0.38),
            tap(0.58, 0.26),
        ]));
"#;

impl SndlabApp {
    pub fn new() -> Self {
        let mut log = LogPane::default();
        let engine = Engine::new().unwrap_or_else(|e| {
            log.error(format!("engine init failed: {e}"));
            panic!("engine init failed: {e}");
        });
        if engine.has_audio() {
            log.info("sndlab ready — F5 to evaluate and play the first patch");
        } else {
            log.warn("audio backend unavailable — playback will be silent");
        }

        Self {
            code: SEED_PATCH.to_string(),
            // Rust syntax is close to Rhai: same comment style,
            // similar keyword set, identical string/number literals.
            // When we ship a Rhai grammar we'll swap this out.
            syntax: Syntax::rust(),
            theme: ColorTheme::AYU_DARK,
            log,
            engine,
            last_buffer: None,
            mcp_endpoint: String::new(),
            filename: "patches.rhai".into(),
        }
    }

    /// Compile the buffer through Rhai, then auto-play the first
    /// patch and capture its rendered buffer for the scope.
    pub fn eval_and_play(&mut self) {
        match self.engine.eval(&self.code) {
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
                    // Snapshot the buffer for the scope before playing
                    // so the scope updates immediately, even if the
                    // audio path stalls.
                    if let Ok(buf) = self.engine.render(&first.name) {
                        self.last_buffer = Some(buf);
                    }
                    match self.engine.play(&first.name) {
                        Ok(()) => self.log.audio(format!(
                            "playing '{}' ({:.2} s)",
                            first.name, first.duration_s
                        )),
                        Err(e) => self.log.error(format!("play failed: {e}")),
                    }
                } else {
                    self.log
                        .warn("no patches registered — script ran but didn't call patch(...)");
                }
            }
            Err(e) => {
                self.log.error(format!("eval failed: {e}"));
            }
        }
    }
}
