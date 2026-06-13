//! sndlab: a TUI sound-design environment with live Rhai scripting,
//! Kira audio playback, and an MCP server so an AI agent can edit the
//! same buffer the user sees.
//!
//! Phase 0: scaffold. The next commits add the TUI shell, the patch DSL,
//! and the MCP transport in that order. See README for the design.

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("sndlab scaffold — no UI yet");
    Ok(())
}
