//! Top-level eframe app state. Holds the editor buffer, the engine,
//! the log, and the last rendered audio buffer (so the scope can show
//! it). Rendering is in `ui::draw`.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui_code_editor::{ColorTheme, Syntax};
use sndlab_core::{Buffer, Engine, PatchRole};

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
    /// Active confirmation / input modal, if any.
    pub modal: Option<Modal>,
    /// One-shot patches the user has "armed" for the scene-fire
    /// button. Triggering the scene plays every armed patch in
    /// parallel through the mixer.
    pub armed: HashSet<String>,
    /// When `true`, the next eval crossfades currently-playing
    /// ambient patches to their newly-rendered buffers automatically,
    /// AND the editor auto-evals after the user stops typing for a
    /// debounce window. When `false`, evals are user-initiated (F5)
    /// and currently-playing ambients keep looping the pre-eval
    /// rendering until manually restarted.
    pub live_ambient: bool,
    /// Live-eval bookkeeping: tracks the last keystroke, the buffer
    /// content at the last successful eval, and the currently-
    /// displayed compile/runtime error.
    pub live: LiveEvalState,
}

#[derive(Debug, Default)]
pub struct LiveEvalState {
    /// Timestamp of the most recent buffer edit, if one is pending
    /// the debounce window. `None` when nothing's pending.
    pub last_change: Option<Instant>,
    /// The buffer contents at the last successful (or attempted)
    /// eval — used to skip work when nothing has actually changed.
    pub last_eval_source: String,
    /// The most recent failure, if the buffer doesn't currently
    /// compile / evaluate cleanly. Drawn as a banner above the
    /// editor; logged only after persisting for a while so a
    /// half-typed expression doesn't fill the log pane.
    pub error: Option<LiveError>,
}

#[derive(Debug, Clone)]
pub struct LiveError {
    pub message: String,
    pub started_at: Instant,
    /// `true` once the error has been written to the log pane.
    /// Prevents repeated logging while the same error persists.
    pub logged: bool,
    /// Source-location detail used to draw the offending line and
    /// caret in the banner. `None` when Rhai didn't give us a
    /// position (rare).
    pub context: Option<ErrorContext>,
}

#[derive(Debug, Clone)]
pub struct ErrorContext {
    pub filename: String,
    pub line: usize,
    pub column: usize,
    pub source_line: String,
}

/// A blocking confirmation or input dialog rendered on top of the
/// normal UI. The user dismisses it with a button before the rest
/// of the toolbar / editor accepts input.
#[derive(Debug, Clone)]
pub enum Modal {
    /// Asking the user to confirm discarding unsaved changes for an
    /// action that replaces the project.
    ConfirmDiscard { action: PendingAction },
    /// Editing a script's filename. `index` is the script being
    /// renamed (or `None` for a brand-new script).
    EditFilename {
        index: Option<usize>,
        input: String,
    },
    /// Confirm deleting a script.
    ConfirmDelete { index: usize },
}

/// What to perform after a `ConfirmDiscard` modal is accepted.
#[derive(Debug, Clone, Copy)]
pub enum PendingAction {
    /// Create a fresh in-memory untitled project.
    NewProject,
    /// Open a project from a directory the user picks.
    OpenProject,
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
        // No "starting with an unsaved project" message here — the
        // status bar already shows the active project's name + path,
        // and a misleading line above an `open_project_at_path` call
        // from main was a confusing combination.

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
            modal: None,
            armed: HashSet::new(),
            live_ambient: false,
            live: LiveEvalState::default(),
        }
    }

    /// Toggle an ambient loop. If it's playing, stop it; if it isn't,
    /// start it.
    pub fn toggle_ambient(&mut self, name: &str) {
        if self.engine.is_ambient_playing(name) {
            self.engine.stop_ambient(name);
            self.log.audio(format!("stopped ambient '{name}'"));
        } else {
            match self.engine.play_ambient(name) {
                Ok(()) => self.log.audio(format!("started ambient '{name}'")),
                Err(e) => self.log.error(format!("ambient start failed: {e}")),
            }
        }
    }

    /// Add or remove a one-shot patch from the scene-arm set.
    pub fn toggle_arm(&mut self, name: &str) {
        if !self.armed.remove(name) {
            self.armed.insert(name.to_string());
        }
    }

    /// Trigger every armed one-shot in parallel through the mixer.
    pub fn fire_scene(&mut self) {
        if self.armed.is_empty() {
            self.log.warn("scene fire: nothing armed");
            return;
        }
        let names: Vec<String> = self.armed.iter().cloned().collect();
        self.log
            .audio(format!("scene fire: {} patches", names.len()));
        for name in names {
            self.play_by_name(&name);
        }
    }

    /// After an eval, decide what to do with currently-playing
    /// ambient loops:
    ///
    /// - Patches that no longer exist in the new patch table get
    ///   their handles dropped (with a quick fade).
    /// - Patches that still exist:
    ///   - If `live_ambient` is on, crossfade to the new buffer so
    ///     the user hears their code changes immediately.
    ///   - If `live_ambient` is off, leave the existing loop running
    ///     — Kira keeps playing the pre-eval rendering until the
    ///     user manually toggles.
    fn reconcile_ambient_after_eval(&mut self, known: &HashSet<String>) {
        const CROSSFADE_MS: u64 = 200;
        let playing = self.engine.ambient_names();
        for name in playing {
            if !known.contains(&name) {
                self.engine.stop_ambient_with_fade(&name, CROSSFADE_MS);
                self.log
                    .audio(format!("ambient '{name}' removed — faded out"));
                continue;
            }
            if self.live_ambient {
                if let Err(e) = self.engine.crossfade_ambient(&name, CROSSFADE_MS) {
                    self.log
                        .error(format!("ambient '{name}' crossfade failed: {e}"));
                }
            }
        }
    }

    /// Create a fresh in-memory untitled project. If the current
    /// project has unsaved changes, prompt first via a confirmation
    /// modal; otherwise act immediately.
    pub fn new_project(&mut self) {
        if self.project.is_dirty() {
            self.modal = Some(Modal::ConfirmDiscard {
                action: PendingAction::NewProject,
            });
            return;
        }
        self.replace_with_fresh_project();
    }

    /// Same dirty-check pattern as `new_project`, but routes to the
    /// directory-picker on confirm.
    pub fn open_project(&mut self) {
        if self.project.is_dirty() {
            self.modal = Some(Modal::ConfirmDiscard {
                action: PendingAction::OpenProject,
            });
            return;
        }
        self.pick_and_open_project();
    }

    fn replace_with_fresh_project(&mut self) {
        self.project = crate::project::Project::unsaved("untitled", SEED_PATCH.to_string());
        self.log.info("started a fresh untitled project");
        self.sync_project_root_to_engine();
    }

    /// Push the active project's root directory to the engine so
    /// relative paths in `sample("…")` resolve correctly. Called after
    /// every project mutation that can change the root.
    fn sync_project_root_to_engine(&mut self) {
        self.engine
            .set_project_root(self.project.root.clone());
    }

    /// Called by the UI's ConfirmDiscard modal once the user has
    /// agreed to drop unsaved changes. Routes to the right action.
    pub fn discard_and(&mut self, action: PendingAction) {
        match action {
            PendingAction::NewProject => self.replace_with_fresh_project(),
            PendingAction::OpenProject => self.pick_and_open_project(),
        }
    }

    /// Add a new script to the current project. The caller is
    /// responsible for prompting for the filename; this just runs
    /// the model-side operation and reports to the log.
    pub fn add_script(&mut self, filename: String) {
        match self.project.add_script(filename) {
            Ok(idx) => {
                self.project.active = idx;
                self.log.info(format!(
                    "added script {}",
                    self.project.scripts[idx].relative_path
                ));
            }
            Err(e) => self.log.error(format!("add script failed: {e}")),
        }
    }

    pub fn rename_script(&mut self, index: usize, new_filename: String) {
        match self.project.rename_script(index, new_filename.clone()) {
            Ok(()) => self.log.info(format!(
                "renamed script #{index} to {new_filename}"
            )),
            Err(e) => self.log.error(format!("rename failed: {e}")),
        }
    }

    pub fn delete_script(&mut self, index: usize) {
        let name = self
            .project
            .scripts
            .get(index)
            .map(|s| s.relative_path.clone())
            .unwrap_or_default();
        match self.project.delete_script(index) {
            Ok(()) => self.log.info(format!("deleted script {name}")),
            Err(e) => self.log.error(format!("delete failed: {e}")),
        }
    }

    /// Open a project from a directory picker. Looks for a
    /// `project.ron` first; falls back to "every `.rhai` in the
    /// directory" if no manifest exists. Errors land in the log.
    /// Open a project from an explicit filesystem path. Accepts either
    /// a directory containing `project.ron` or the `project.ron` file
    /// itself, in absolute or relative form. Used by the
    /// `sndlab <path>` CLI entry point so a user can launch sndlab
    /// pointing directly at a project. Failures are logged, not fatal
    /// — the app continues with the untitled project.
    pub fn open_project_at_path(&mut self, path: &std::path::Path) {
        // Resolve relative paths against the process CWD up front, so
        // a relative arg works regardless of the actual cwd at the
        // point of evaluation. Falls back to the raw path if cwd is
        // unavailable.
        let absolute: std::path::PathBuf = if path.is_absolute() {
            path.to_path_buf()
        } else {
            match std::env::current_dir() {
                Ok(cwd) => cwd.join(path),
                Err(_) => path.to_path_buf(),
            }
        };
        // Strip "./" / ".." and confirm the file/dir actually exists.
        // canonicalize() resolves symlinks and returns an absolute
        // path; on failure we get a more specific OS error than the
        // generic "path does not exist".
        let canonical = match std::fs::canonicalize(&absolute) {
            Ok(p) => p,
            Err(e) => {
                let cwd = std::env::current_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| "(unknown)".to_string());
                let msg = format!(
                    "open project failed: {} (resolved to {}, cwd was {})",
                    e,
                    absolute.display(),
                    cwd
                );
                self.log.error(msg.clone());
                eprintln!("sndlab: {msg}");
                return;
            }
        };
        let dir: std::path::PathBuf = if canonical.is_file() {
            canonical
                .parent()
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("."))
        } else {
            canonical
        };
        let manifest_present = dir.join("project.ron").is_file();
        let result = if manifest_present {
            Project::open(&dir)
        } else {
            Project::open_directory(&dir)
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
                eprintln!(
                    "sndlab: opened project '{name}' ({n} script{}) from {}",
                    if n == 1 { "" } else { "s" },
                    dir.display()
                );
                self.sync_project_root_to_engine();
                // Republish the new active buffer so an MCP client that
                // attached during startup sees the project's content,
                // not the seed script.
                if let Some(mailbox) = self.mailbox.as_ref() {
                    if let Ok(mut m) = mailbox.lock() {
                        m.current_buffer = self.project.active_buffer().to_string();
                    }
                }
            }
            Err(e) => {
                let msg = format!(
                    "open project failed: {e} (dir: {})",
                    dir.display()
                );
                self.log.error(msg.clone());
                eprintln!("sndlab: {msg}");
            }
        }
    }

    pub fn pick_and_open_project(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Open project directory");
        if let Ok(cwd) = std::env::current_dir() {
            dialog = dialog.set_directory(cwd);
        }
        let Some(path) = dialog.pick_folder() else {
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
                self.sync_project_root_to_engine();
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
        let mut dialog = rfd::FileDialog::new().set_title("Save project as");
        if let Ok(cwd) = std::env::current_dir() {
            dialog = dialog.set_directory(cwd);
        }
        let Some(path) = dialog.pick_folder() else {
            return;
        };
        match self.project.save_to(&path) {
            Ok(()) => {
                self.log.info(format!("saved {}", path.display()));
                self.sync_project_root_to_engine();
            }
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
                    // Treat MCP edits the same as user typing for
                    // the live-eval debounce: they restart the timer
                    // so if the AI keeps editing in quick succession
                    // we only re-eval once the dust settles.
                    self.note_typed();
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
        let playing_ambients = self.engine.ambient_names();
        let live_ambient = self.live_ambient;
        let mut m = mailbox.lock().unwrap();
        m.current_buffer = active_buffer;
        m.current_patches = patches_snapshot;
        m.playing_ambients = playing_ambients;
        m.live_ambient = live_ambient;
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
        let mut dialog = rfd::FileDialog::new()
            .add_filter("audio", &["mp3", "wav", "ogg", "flac"])
            .add_filter("any", &["*"])
            .set_title("Load reference audio");
        // Prefer the project root, falling back to CWD; both are far
        // more likely to contain the sample the user wants than $HOME.
        let start_dir = self
            .project
            .root
            .clone()
            .or_else(|| std::env::current_dir().ok());
        if let Some(d) = start_dir {
            dialog = dialog.set_directory(d);
        }
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

    /// Manual eval (F5 / "Eval + Play" button) — compiles every
    /// script, reconciles ambient handles, then auto-plays the first
    /// one-shot patch so the user gets immediate audible feedback.
    pub fn eval_and_play(&mut self) {
        let played = self.eval_only(true);
        if !played {
            // eval_only logged the failure (manual eval is verbose).
        }
    }

    /// Note that the user typed (used by the live-eval debounce).
    /// Called from the UI's editor render whenever the buffer
    /// changes, sets a fresh timer; until the debounce window
    /// elapses without further keystrokes no auto-eval runs.
    pub fn note_typed(&mut self) {
        self.live.last_change = Some(Instant::now());
    }

    /// Called from the eframe update loop. If `live_ambient` is on
    /// and the user has stopped typing for the debounce window, run
    /// an auto-eval. If a live error is active and has persisted
    /// long enough, log it to the log pane.
    pub fn pump_live_eval(&mut self) {
        const DEBOUNCE: Duration = Duration::from_millis(400);
        const ERROR_LOG_DELAY: Duration = Duration::from_secs(3);

        if self.live_ambient {
            if let Some(t) = self.live.last_change {
                if Instant::now().duration_since(t) >= DEBOUNCE {
                    // Buffer might be unchanged from last_eval if the
                    // user typed and reverted; auto_eval still re-runs
                    // (cheap) but we'll skip the reconcile if the
                    // source hasn't actually changed.
                    let current = self.project.concatenated_source();
                    if current != self.live.last_eval_source {
                        self.auto_eval();
                    }
                    self.live.last_change = None;
                }
            }
        }

        // Promote a persistent live error to the log pane once.
        if let Some(err) = &mut self.live.error {
            if !err.logged && err.started_at.elapsed() >= ERROR_LOG_DELAY {
                let msg = err.message.clone();
                err.logged = true;
                self.log.error(format!("script still failing: {msg}"));
            }
        }
    }

    /// Auto-eval driven by the typing debounce. Compiles every script
    /// and reconciles ambient handles, but does NOT trigger a one-
    /// shot — auto-firing the ping every time you type a character
    /// is loud and annoying. The user keeps F5 / the toolbar button
    /// for that.
    pub fn auto_eval(&mut self) {
        let _ = self.eval_only(false);
    }

    /// Single-source eval implementation. `manual` controls whether
    /// successes/failures are logged immediately (manual evals log
    /// loudly; auto-evals defer logging via the live-eval state) and
    /// whether the first one-shot fires. Returns `true` if a one-
    /// shot was played.
    fn eval_only(&mut self, manual: bool) -> bool {
        let source = self.project.concatenated_source();
        self.live.last_eval_source = source.clone();
        match self.engine.eval(&source) {
            Ok(summary) => {
                self.live.error = None;
                if manual {
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
                }
                self.set_last_error(None);
                let known: HashSet<String> =
                    summary.patches.iter().map(|p| p.name.clone()).collect();
                self.armed.retain(|n| known.contains(n));
                self.reconcile_ambient_after_eval(&known);
                if manual {
                    if let Some(first) = summary
                        .patches
                        .iter()
                        .find(|p| p.role == PatchRole::OneShot)
                    {
                        let name = first.name.clone();
                        self.play_by_name(&name);
                        return true;
                    } else if summary.patches.is_empty() {
                        self.log.warn(
                            "no patches registered — script ran but didn't call patch(...)",
                        );
                    }
                }
                false
            }
            Err(e) => {
                let position = match &e {
                    sndlab_core::Error::Parse { position, .. }
                    | sndlab_core::Error::Runtime { position, .. } => *position,
                    _ => None,
                };
                let context = position.and_then(|p| {
                    let (script_idx, local_line) =
                        self.project.resolve_source_line(p.line)?;
                    let source_line =
                        self.project.script_line(script_idx, local_line)?;
                    Some(ErrorContext {
                        filename: self
                            .project
                            .scripts
                            .get(script_idx)
                            .map(|s| s.relative_path.clone())
                            .unwrap_or_default(),
                        line: local_line,
                        column: p.column,
                        source_line,
                    })
                });
                let msg = format!("{e}");
                let already_same_error = matches!(
                    &self.live.error,
                    Some(existing) if existing.message == msg
                );
                if !already_same_error {
                    self.live.error = Some(LiveError {
                        message: msg.clone(),
                        started_at: Instant::now(),
                        logged: false,
                        context,
                    });
                }
                if manual {
                    self.log.error(format!("eval failed: {msg}"));
                    self.set_last_error(Some(msg));
                    // The error's already been logged manually; mark
                    // it so the debounce doesn't re-log it.
                    if let Some(err) = &mut self.live.error {
                        err.logged = true;
                    }
                } else {
                    self.set_last_error(Some(msg));
                }
                false
            }
        }
    }
}
