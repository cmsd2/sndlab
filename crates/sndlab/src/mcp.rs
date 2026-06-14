//! MCP server. Exposes the editor buffer and the engine over the
//! Model Context Protocol so an AI agent (Claude Code, etc.) can
//! collaborate on the same script the user is editing.
//!
//! ## Threading
//!
//! eframe owns the main thread. rmcp wants a tokio runtime. We spawn
//! a dedicated thread that owns the runtime; it talks to the main
//! thread via an `Arc<Mutex<Mailbox>>`. The mailbox carries:
//!
//! - **main → MCP** snapshots of the current editor buffer, the
//!   registered patches, and the last error. These let `get_buffer`,
//!   `list_patches`, and `last_error` answer synchronously without
//!   reaching into the engine across threads.
//! - **MCP → main** pending commands (replace the buffer, re-eval,
//!   play a patch by name). The main thread drains these once per
//!   frame and executes them against its owned engine.
//!
//! Locks are held only briefly on either side. No tool call ever
//! waits on egui's rendering and no egui frame ever waits on a tool
//! call.

use std::sync::{Arc, Mutex};

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ServerHandler, schemars, tool, tool_handler, tool_router};
use serde::Deserialize;
use sndlab_core::PatchInfo;

/// Shared state between the eframe main thread and the tokio
/// MCP-server thread.
#[derive(Debug, Default)]
pub struct Mailbox {
    /// Current editor buffer, updated by the main thread each frame.
    pub current_buffer: String,
    /// Current registered patches, updated by the main thread after
    /// each eval.
    pub current_patches: Vec<PatchInfo>,
    /// Most recent eval/play error, updated by the main thread.
    pub last_error: Option<String>,
    /// True when the user has enabled "Live" mode — auto-eval after
    /// debounce, crossfade ambients into the new buffer on success.
    /// Hint to the AI that calling play() on a looping ambient is
    /// redundant; edits are heard automatically.
    pub live_ambient: bool,
    /// Names of ambient patches currently looping through the mixer.
    pub playing_ambients: Vec<String>,

    /// Pending commands from MCP to be processed on the main thread.
    pub pending: Vec<Command>,
}

impl Mailbox {
    pub fn new() -> Self {
        Self::default()
    }
}

/// A request from the MCP side that needs to run on the main thread
/// (where the engine and editor live).
#[derive(Debug, Clone)]
pub enum Command {
    /// Replace the editor buffer with this content.
    SetBuffer(String),
    /// Re-evaluate the buffer (compile + replace patch table).
    Eval,
    /// Play a registered patch by name.
    Play(String),
}

/// The rmcp server. Carries the mailbox as shared state.
#[derive(Clone)]
pub struct Server {
    mailbox: Arc<Mutex<Mailbox>>,
    // Populated and consumed by rmcp's `#[tool_router]` / `#[tool_handler]`
    // macros, which the compiler's dead-code pass can't see.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetBufferArgs {
    #[schemars(description = "The new contents of the editor buffer, replacing what's there.")]
    content: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ApplyEditArgs {
    #[schemars(
        description = "Exact substring to find. Must occur exactly once in the buffer; if it occurs zero or many times the edit fails."
    )]
    old_string: String,
    #[schemars(description = "Replacement text.")]
    new_string: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PlayArgs {
    #[schemars(
        description = "Name of the registered patch to play. Call list_patches first if unsure."
    )]
    name: String,
}

#[tool_router]
impl Server {
    pub fn new(mailbox: Arc<Mutex<Mailbox>>) -> Self {
        Self {
            mailbox,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Return the current contents of the editor buffer. Use this before making any edit so you're operating on the user's current state."
    )]
    fn get_buffer(&self) -> String {
        let m = self.mailbox.lock().unwrap();
        m.current_buffer.clone()
    }

    #[tool(
        description = "Replace the entire editor buffer with new content. Prefer apply_edit for small changes so the user can see what you changed."
    )]
    fn set_buffer(&self, Parameters(args): Parameters<SetBufferArgs>) -> String {
        let mut m = self.mailbox.lock().unwrap();
        m.pending.push(Command::SetBuffer(args.content));
        "ok".into()
    }

    #[tool(
        description = "Find `old_string` in the buffer exactly once and replace it with `new_string`. Fails if old_string isn't found or occurs more than once."
    )]
    fn apply_edit(&self, Parameters(args): Parameters<ApplyEditArgs>) -> String {
        let mut m = self.mailbox.lock().unwrap();
        let buffer = m.current_buffer.clone();
        let count = buffer.matches(&args.old_string).count();
        if count == 0 {
            return "error: old_string not found in buffer".into();
        }
        if count > 1 {
            return format!(
                "error: old_string occurs {count} times; needs more surrounding context to disambiguate"
            );
        }
        let new_buffer = buffer.replacen(&args.old_string, &args.new_string, 1);
        m.pending.push(Command::SetBuffer(new_buffer));
        "ok".into()
    }

    #[tool(
        description = "List all patches registered by the most recent successful eval. Returns one line per patch: 'name (role, duration_s)'."
    )]
    fn list_patches(&self) -> String {
        let m = self.mailbox.lock().unwrap();
        if m.current_patches.is_empty() {
            return "(no patches registered — call eval after editing)".into();
        }
        m.current_patches
            .iter()
            .map(|p| {
                let role = match p.role {
                    sndlab_core::PatchRole::OneShot => "one_shot",
                    sndlab_core::PatchRole::Ambient => "ambient",
                };
                format!("{} ({}, {:.2} s)", p.name, role, p.duration_s)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[tool(
        description = "Re-evaluate the current editor buffer. Any patches registered by the script become available to play. Errors land in last_error."
    )]
    fn eval(&self) -> String {
        let mut m = self.mailbox.lock().unwrap();
        m.pending.push(Command::Eval);
        "ok (queued for next frame)".into()
    }

    #[tool(
        description = "Play a registered patch by name once as a one-shot. Audio plays out the user's speakers; you don't receive the audio yourself. \
                       NOTE: this always fires a one-shot — even for ambient-role patches. If an ambient is already looping (check engine_state) calling play creates a duplicate one-shot on top of the loop, which is almost never what you want. Don't call play on ambient patches after editing them when Live mode is on; the loop crossfades into your edits automatically. For one-shots (sonar pings, hit sounds), call play freely after each edit so the user hears the result."
    )]
    fn play(&self, Parameters(args): Parameters<PlayArgs>) -> String {
        let mut m = self.mailbox.lock().unwrap();
        m.pending.push(Command::Play(args.name));
        "ok (queued for next frame)".into()
    }

    #[tool(
        description = "Return the engine's current playback and live-mode state. Use this before deciding whether to call play(): if a patch is in 'playing_ambients' and live_ambient is true, your buffer edits are already audible without a play call."
    )]
    fn engine_state(&self) -> String {
        let m = self.mailbox.lock().unwrap();
        let mut out = String::new();
        out.push_str(&format!("live_ambient: {}\n", m.live_ambient));
        if m.playing_ambients.is_empty() {
            out.push_str("playing_ambients: (none)\n");
        } else {
            out.push_str("playing_ambients:\n");
            for n in &m.playing_ambients {
                out.push_str(&format!("  - {n}\n"));
            }
        }
        out
    }

    #[tool(
        description = "Return the most recent error from eval or play, or 'no error' if everything is happy."
    )]
    fn last_error(&self) -> String {
        let m = self.mailbox.lock().unwrap();
        match &m.last_error {
            Some(e) => e.clone(),
            None => "no error".into(),
        }
    }
}

#[tool_handler]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "sndlab editor + audio engine. Tools let you read the editor buffer, propose edits, re-evaluate the buffer, and play patches. \
                 \n\nAudio plays from the user's machine; you don't hear it directly — ask them what it sounded like. \
                 \n\nPatches have a role: 'one_shot' fires once when triggered (sonar pings, weapon launches, UI clicks), 'ambient' loops continuously in the background (ocean rumble, machinery hum). The user toggles ambient loops on/off in the toolbar. \
                 \n\nThe user can enable 'Live' mode (visible in engine_state as live_ambient: true). In Live mode, sndlab auto-evals after each typing burst and crossfades currently-playing ambient loops into the new rendering automatically. \
                 \n\nDecision rule for what to call after editing: \
                 \n- For one-shot patches: call play(name) so the user hears the result. \
                 \n- For ambient patches: check engine_state. If the ambient is in playing_ambients AND live_ambient is true, do nothing — the loop already crossfaded into your edits. If it's not playing or live_ambient is false, mention it in your reply so the user can toggle it manually. Don't call play() on a looping ambient; it fires a one-shot duplicate on top of the loop.",
            )
    }
}

/// Spawn a thread that owns a tokio runtime and serves the MCP API
/// over streamable HTTP on `127.0.0.1:port`. Returns the join handle
/// so the caller can keep it alive; if the handle is dropped the
/// server keeps running because the thread holds its own resources.
pub fn spawn_server(mailbox: Arc<Mutex<Mailbox>>, port: u16) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                tracing::error!("mcp: failed to start tokio runtime: {e}");
                return;
            }
        };
        runtime.block_on(async move {
            let mailbox_factory = mailbox.clone();
            let service = StreamableHttpService::new(
                move || Ok(Server::new(mailbox_factory.clone())),
                std::sync::Arc::new(LocalSessionManager::default()),
                StreamableHttpServerConfig::default(),
            );
            let router = axum::Router::new().nest_service("/mcp", service);
            let addr = format!("127.0.0.1:{port}");
            let listener = match tokio::net::TcpListener::bind(&addr).await {
                Ok(l) => l,
                Err(e) => {
                    tracing::error!("mcp: failed to bind {addr}: {e}");
                    return;
                }
            };
            tracing::info!("mcp: serving on http://{addr}/mcp");
            if let Err(e) = axum::serve(listener, router).await {
                tracing::error!("mcp: server exited with error: {e}");
            }
        });
    })
}
