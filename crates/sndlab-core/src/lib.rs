//! sndlab-core: the reusable audio engine.
//!
//! Wraps a Rhai script engine and a Kira audio manager. Scripts register
//! named *patches* via a small DSL (`sine`, `noise`, `env`, `gain`, `mix`,
//! `tap`, `patch`); the host evaluates the script, then asks the engine
//! to play patches by name.
//!
//! This crate has no UI, no MCP, no project model. Those live in the
//! `sndlab` binary. The split exists so games and batch tools can embed
//! this engine without dragging in a TUI.
//!
//! # Status
//!
//! Skeleton — the API surface is shaped here but the implementation
//! arrives in subsequent commits (DSL registration, Rhai-to-buffer
//! render, Kira playback).

use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("rhai parse error: {0}")]
    Parse(String),
    #[error("rhai runtime error: {0}")]
    Runtime(String),
    #[error("audio backend error: {0}")]
    Audio(String),
    #[error("no patch named '{0}'")]
    UnknownPatch(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// How a patch is meant to be used by the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PatchRole {
    /// One-shot: triggered explicitly, plays once, dies.
    OneShot,
    /// Ambient: loop continuously while the project is open, modulated
    /// by host-provided parameters.
    Ambient,
}

#[derive(Debug, Clone)]
pub struct PatchInfo {
    pub name: String,
    pub role: PatchRole,
    pub duration_s: f32,
}

/// Summary of what changed after an `Engine::eval` call.
#[derive(Debug, Default, Clone)]
pub struct EvalSummary {
    pub patches: Vec<PatchInfo>,
    /// Free-form notes from the host (e.g. parse warnings).
    pub messages: Vec<String>,
}

/// The audio engine. Holds the live Kira manager and the registered
/// patches.
pub struct Engine {
    // Placeholders until the implementation lands.
    _placeholder: (),
}

impl Engine {
    /// Construct a new engine and try to bring up the audio backend.
    /// Returns an `Audio` error if the OS audio device cannot be opened;
    /// the caller may choose to fall back to a silent mode.
    pub fn new() -> Result<Self> {
        Ok(Self { _placeholder: () })
    }

    /// Evaluate a script and replace the engine's set of patches with
    /// whatever the script registered. Atomic on success.
    pub fn eval(&mut self, _source: &str) -> Result<EvalSummary> {
        Ok(EvalSummary::default())
    }

    /// Play a patch by name. Returns immediately; audio runs on its own
    /// thread driven by Kira/cpal.
    pub fn play(&mut self, name: &str) -> Result<()> {
        Err(Error::UnknownPatch(name.into()))
    }

    /// Currently-registered patches, in registration order.
    pub fn patches(&self) -> &[PatchInfo] {
        &[]
    }
}

/// A baked audio buffer: mono samples plus the sample rate they were
/// rendered at. The TUI uses this to display a scope; the host uses it
/// to play through Kira.
#[derive(Debug, Clone)]
pub struct Buffer {
    pub sample_rate: u32,
    pub samples: Arc<[f32]>,
}
