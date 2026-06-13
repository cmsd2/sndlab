//! Top-level eframe app state. Holds the editor buffer, the engine,
//! the log, and the last rendered audio buffer (so the scope can show
//! it). Rendering is in `ui::draw`.

use std::sync::{Arc, Mutex};

use egui_code_editor::{ColorTheme, Syntax};
use sndlab_core::{Buffer, Engine};

use crate::log::LogPane;
use crate::mcp::{Command, Mailbox};

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
    /// Endpoint string for the status bar.
    pub mcp_endpoint: String,
    pub filename: String,
    /// Shared state with the MCP server thread. `None` if the MCP
    /// server failed to start; the app then runs single-user.
    pub mailbox: Option<Arc<Mutex<Mailbox>>>,
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

/// TCP port the MCP server binds to on 127.0.0.1.
const MCP_PORT: u16 = 7777;

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

        // Spin up the MCP server. The mailbox is the shared state
        // between this thread (which owns the engine + editor) and
        // the tokio thread that serves HTTP. Failure to spawn the
        // server doesn't sink the app — it just means no AI collab.
        let mailbox = Arc::new(Mutex::new(Mailbox::new()));
        {
            let m = mailbox.clone();
            crate::mcp::spawn_server(m, MCP_PORT);
        }
        // Seed the mailbox snapshot so an MCP client that connects
        // before the first frame draws still sees current state.
        if let Ok(mut m) = mailbox.lock() {
            m.current_buffer = SEED_PATCH.to_string();
        }
        log.info(format!("MCP server at http://127.0.0.1:{MCP_PORT}/mcp"));

        Self {
            code: SEED_PATCH.to_string(),
            syntax: Syntax::rust(),
            theme: ColorTheme::AYU_DARK,
            log,
            engine,
            last_buffer: None,
            mcp_endpoint: format!("http://127.0.0.1:{MCP_PORT}/mcp"),
            filename: "patches.rhai".into(),
            mailbox: Some(mailbox),
        }
    }

    /// Each frame, drain any commands the MCP thread queued and
    /// publish the engine's current state back so the next MCP read
    /// sees fresh data. Lock is held only briefly on either side.
    pub fn pump_mailbox(&mut self) {
        // Clone the Arc up front so we don't hold a borrow on self
        // while calling its other methods.
        let mailbox = match self.mailbox.as_ref() {
            Some(m) => m.clone(),
            None => return,
        };

        // Take all pending commands out of the mailbox in one lock
        // window. Execute them outside the lock so MCP calls don't
        // block on engine work.
        let pending: Vec<Command> = {
            let mut m = mailbox.lock().unwrap();
            std::mem::take(&mut m.pending)
        };

        for cmd in pending {
            match cmd {
                Command::SetBuffer(content) => {
                    self.code = content;
                    self.log.info("MCP applied buffer edit");
                }
                Command::Eval => self.eval_and_play(),
                Command::Play(name) => self.play_by_name(&name),
            }
        }

        // Publish current state back to the mailbox.
        let patches_snapshot = self.engine.patches().to_vec();
        let mut m = mailbox.lock().unwrap();
        m.current_buffer = self.code.clone();
        m.current_patches = patches_snapshot;
    }

    /// Play a specific patch (used by toolbar buttons and by the
    /// MCP `play` tool). Reports through the log and last_error.
    pub fn play_by_name(&mut self, name: &str) {
        if let Ok(buf) = self.engine.render(name) {
            self.last_buffer = Some(buf);
        }
        match self.engine.play(name) {
            Ok(()) => {
                self.log.audio(format!("playing '{}'", name));
                self.set_last_error(None);
            }
            Err(e) => {
                let msg = format!("play '{name}' failed: {e}");
                self.log.error(msg.clone());
                self.set_last_error(Some(msg));
            }
        }
    }

    fn set_last_error(&mut self, err: Option<String>) {
        if let Some(mailbox) = &self.mailbox {
            if let Ok(mut m) = mailbox.lock() {
                m.last_error = err;
            }
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
                self.set_last_error(None);
                if let Some(first) = summary.patches.first() {
                    let name = first.name.clone();
                    self.play_by_name(&name);
                } else {
                    self.log
                        .warn("no patches registered — script ran but didn't call patch(...)");
                }
            }
            Err(e) => {
                let msg = format!("eval failed: {e}");
                self.log.error(msg.clone());
                self.set_last_error(Some(msg));
            }
        }
    }
}
