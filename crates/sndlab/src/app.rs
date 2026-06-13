//! Top-level eframe app state. Holds the editor buffer, the engine,
//! the log, and the last rendered audio buffer (so the scope can show
//! it). Rendering is in `ui::draw`.

use std::sync::{Arc, Mutex};

use egui_code_editor::{ColorTheme, Syntax};
use sndlab_core::{Buffer, Engine};

use crate::log::LogPane;
use crate::mcp::{Command, Mailbox};
use crate::project::Project;
use crate::spectrum;

pub struct SndlabApp {
    /// The currently-loaded project. Always present — at startup we
    /// build an in-memory "untitled" project so the user has
    /// something to edit and the editor pane is never blank.
    pub project: Project,
    /// What egui_code_editor uses for syntax highlighting. Rust is
    /// close enough to Rhai visually for now.
    pub syntax: Syntax,
    pub theme: ColorTheme,
    pub log: LogPane,
    pub engine: Engine,
    /// The most recently rendered patch buffer — what the scope's
    /// upper pane shows.
    pub last_buffer: Option<Buffer>,
    /// FFT magnitudes of `last_buffer`, computed when it changes.
    /// What the scope's lower pane shows.
    pub last_spectrum: Option<Vec<f32>>,
    /// Optional reference audio file decoded from disk and overlaid
    /// on the scope so designers can A/B their patch against a
    /// recording. `None` until "Load reference..." is used.
    pub reference_buffer: Option<Buffer>,
    pub reference_spectrum: Option<Vec<f32>>,
    /// Filename of the loaded reference, for the toolbar/status display.
    pub reference_name: Option<String>,
    /// Endpoint string for the status bar.
    pub mcp_endpoint: String,
    /// Shared state with the MCP server thread. `None` if the MCP
    /// server failed to start; the app then runs single-user.
    pub mailbox: Option<Arc<Mutex<Mailbox>>>,
}

const SEED_PATCH: &str = r#"// Press F5 to evaluate and play the first patch.
//
// chirp(start_hz, end_hz, dur_s) sweeps frequency over the buffer.
// A swept source is broadband, so summed delayed copies (taps) no
// longer lock into a sustained comb-filter null at any single
// frequency — the interference moves with time and the result
// reads as reverb rather than phaser. The scope's lower pane shows
// the spread of frequencies the chirp covers.

patch("ping", "one_shot",
    chirp(280.0, 400.0, 1.0).env(0.008, 1.4).gain(0.32)
        .with_taps([
            tap(0.13, 0.7),
            tap(0.31, 0.5),
            tap(0.58, 0.35),
            tap(0.95, 0.22),
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
        let project = Project::unsaved("untitled", SEED_PATCH.to_string());
        // Seed the mailbox snapshot so an MCP client that connects
        // before the first frame draws still sees current state.
        if let Ok(mut m) = mailbox.lock() {
            m.current_buffer = project.active_buffer().to_string();
        }
        log.info(format!("MCP server at http://127.0.0.1:{MCP_PORT}/mcp"));
        log.info("starting with an unsaved project — Open... or Save As... to persist");

        Self {
            project,
            syntax: Syntax::rust(),
            theme: ColorTheme::AYU_DARK,
            log,
            engine,
            last_buffer: None,
            last_spectrum: None,
            reference_buffer: None,
            reference_spectrum: None,
            reference_name: None,
            mcp_endpoint: format!("http://127.0.0.1:{MCP_PORT}/mcp"),
            mailbox: Some(mailbox),
        }
    }

    /// Open a project from a directory picker. Looks for a
    /// `project.ron` first; falls back to "every `.rhai` in the
    /// directory" if no manifest exists. Errors land in the log.
    pub fn pick_and_open_project(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Open project directory")
            .pick_folder()
        else {
            return;
        };
        let manifest_present = path.join("project.ron").is_file();
        let result = if manifest_present {
            Project::open(&path)
        } else {
            Project::open_directory(&path)
        };
        match result {
            Ok(project) => {
                let name = project.manifest.name.clone();
                let n = project.scripts.len();
                self.project = project;
                self.log.info(format!(
                    "opened project '{name}' ({n} script{})",
                    if n == 1 { "" } else { "s" }
                ));
            }
            Err(e) => {
                self.log.error(format!("open project failed: {e}"));
            }
        }
    }

    /// Save the current project. Falls through to Save As... if the
    /// project has no root directory yet.
    pub fn save_project(&mut self) {
        if self.project.root.is_none() {
            self.save_project_as();
            return;
        }
        match self.project.save() {
            Ok(()) => self.log.info(format!(
                "saved {}",
                self.project
                    .root
                    .as_ref()
                    .and_then(|p| p.to_str())
                    .unwrap_or("?")
            )),
            Err(e) => self.log.error(format!("save failed: {e}")),
        }
    }

    /// Pick a destination directory and save the project there. Sets
    /// the project's root so subsequent saves go to the same place.
    pub fn save_project_as(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save project as")
            .pick_folder()
        else {
            return;
        };
        match self.project.save_to(&path) {
            Ok(()) => self
                .log
                .info(format!("saved {}", path.display())),
            Err(e) => self.log.error(format!("save failed: {e}")),
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
                    self.project.set_active_buffer(content);
                    self.log.info("MCP applied buffer edit");
                }
                Command::Eval => self.eval_and_play(),
                Command::Play(name) => self.play_by_name(&name),
            }
        }

        // Publish current state back to the mailbox. The "current
        // buffer" the MCP sees is the active script — the AI edits
        // whichever file the user is looking at.
        let patches_snapshot = self.engine.patches().to_vec();
        let active_buffer = self.project.active_buffer().to_string();
        let mut m = mailbox.lock().unwrap();
        m.current_buffer = active_buffer;
        m.current_patches = patches_snapshot;
    }

    /// Play a specific patch (used by toolbar buttons and by the
    /// MCP `play` tool). Reports through the log and last_error.
    pub fn play_by_name(&mut self, name: &str) {
        if let Ok(buf) = self.engine.render(name) {
            self.set_last_buffer(buf);
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

    /// Update the cached buffer and (re)compute its spectrum. Single
    /// site so the spectrum is always in sync with the waveform.
    fn set_last_buffer(&mut self, buf: Buffer) {
        self.last_spectrum = Some(spectrum::compute(&buf));
        self.last_buffer = Some(buf);
    }

    /// Open a file picker and load the chosen audio file as the
    /// reference. Errors land in the log; success replaces any
    /// previously-loaded reference. Runs the file dialog
    /// synchronously, which briefly blocks the UI thread — fine
    /// because it's user-initiated.
    pub fn pick_and_load_reference(&mut self) {
        let dialog = rfd::FileDialog::new()
            .add_filter("audio", &["mp3", "wav", "ogg", "flac"])
            .add_filter("any", &["*"])
            .set_title("Load reference audio");
        let Some(path) = dialog.pick_file() else {
            return;
        };
        let display = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("(unnamed)")
            .to_string();
        match crate::reference::load(&path) {
            Ok(buf) => {
                self.reference_spectrum = Some(spectrum::compute(&buf));
                self.reference_buffer = Some(buf);
                self.reference_name = Some(display.clone());
                self.log.info(format!("loaded reference: {display}"));
            }
            Err(e) => {
                self.log.error(format!("reference load failed: {e}"));
            }
        }
    }

    pub fn clear_reference(&mut self) {
        if self.reference_buffer.take().is_some() {
            self.reference_spectrum = None;
            self.reference_name = None;
            self.log.info("reference cleared");
        }
    }

    /// Compile every script in the project through Rhai (as a single
    /// concatenated source so the patch namespace is shared across
    /// files), then auto-play the first patch and capture its
    /// rendered buffer for the scope.
    pub fn eval_and_play(&mut self) {
        let source = self.project.concatenated_source();
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
