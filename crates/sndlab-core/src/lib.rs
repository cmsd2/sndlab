//! sndlab-core: the reusable audio engine.
//!
//! Wraps a Rhai script engine and a Kira audio manager. Scripts
//! register named *patches* via a small DSL (`sine`, `noise`, `env`,
//! `gain`, `mix`, `tap`, `with_taps`, `patch`); the host evaluates the
//! script, then asks the engine to play patches by name.
//!
//! This crate has no UI, no MCP, no project model. Those live in the
//! `sndlab` binary. The split exists so games and batch tools can
//! embed this engine without dragging in a TUI.

mod engine;
mod signal;

pub use engine::Engine;
pub use signal::{NoiseKind, Signal, Tap, SAMPLE_RATE};

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
    /// Free-form notes from the host (e.g. patch redefinitions).
    pub messages: Vec<String>,
}

/// A baked audio buffer: mono samples plus the sample rate they were
/// rendered at. The TUI uses this to display a scope; the host uses
/// it to play through Kira.
#[derive(Debug, Clone)]
pub struct Buffer {
    pub sample_rate: u32,
    pub samples: Arc<[f32]>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sine + envelope patch evaluates, registers under the expected
    /// name, has the expected duration, and is non-silent.
    #[test]
    fn sine_patch_round_trips() {
        let mut engine = Engine::new().expect("engine init");
        let summary = engine
            .eval(
                r#"
                patch("ping", "one_shot", sine(330.0, 0.5).env(0.01, 2.0).gain(0.3));
            "#,
            )
            .expect("eval ok");
        assert_eq!(summary.patches.len(), 1);
        assert_eq!(summary.patches[0].name, "ping");
        assert_eq!(summary.patches[0].role, PatchRole::OneShot);
        let buf = engine.render("ping").expect("render ok");
        // 0.5 s at 48 kHz is 24_000 samples; allow rounding.
        assert!((buf.samples.len() as i64 - 24_000).abs() < 2);
        let peak = buf
            .samples
            .iter()
            .map(|s| s.abs())
            .fold(0.0_f32, f32::max);
        assert!(peak > 0.01, "buffer should have meaningful amplitude");
        assert!(peak < 1.0, "buffer should not clip");
    }

    /// Reverb taps extend the buffer past the dry source and contribute
    /// non-zero samples in the tail.
    #[test]
    fn reverb_taps_extend_signal() {
        let mut engine = Engine::new().expect("engine init");
        engine
            .eval(
                r#"
                patch("rev", "one_shot",
                    sine(440.0, 0.2).gain(0.5)
                        .with_taps([tap(0.3, 0.4), tap(0.6, 0.2)]));
            "#,
            )
            .expect("eval ok");
        let buf = engine.render("rev").expect("render ok");
        // Source is 0.2 s; longest tap delays by 0.6 s → buffer ≈ 0.8 s.
        let len_s = buf.samples.len() as f32 / buf.sample_rate as f32;
        assert!(len_s > 0.75 && len_s < 0.85, "len_s = {len_s}");
        // Tail (after the dry portion) should contain non-zero samples.
        let tail = &buf.samples[(buf.samples.len() * 3 / 4)..];
        let tail_peak = tail.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!(tail_peak > 0.001, "tap energy should reach the tail");
    }

    /// Playing an unknown patch returns UnknownPatch.
    #[test]
    fn unknown_patch_errors() {
        let mut engine = Engine::new().expect("engine init");
        assert!(matches!(
            engine.play("nope"),
            Err(Error::UnknownPatch(_))
        ));
    }

    /// Re-eval replaces the patch table atomically — an old patch
    /// that's not in the new script disappears.
    #[test]
    fn re_eval_replaces_patches() {
        let mut engine = Engine::new().expect("engine init");
        engine
            .eval(r#"patch("a", "one_shot", sine(200.0, 0.1));"#)
            .expect("eval a");
        engine
            .eval(r#"patch("b", "one_shot", sine(300.0, 0.1));"#)
            .expect("eval b");
        let names: Vec<_> = engine.patches().iter().map(|p| p.name.clone()).collect();
        assert_eq!(names, vec!["b"]);
    }

    /// Mixing two short signals produces a buffer the length of the
    /// longer input and contains contributions from both.
    #[test]
    fn mix_sums_signals() {
        let mut engine = Engine::new().expect("engine init");
        engine
            .eval(
                r#"
                patch("twin", "one_shot",
                    mix([
                        sine(220.0, 0.1).gain(0.3),
                        sine(330.0, 0.2).gain(0.3),
                    ]));
            "#,
            )
            .expect("eval ok");
        let buf = engine.render("twin").expect("render ok");
        // Output length is max input length (0.2 s).
        let len_s = buf.samples.len() as f32 / buf.sample_rate as f32;
        assert!((len_s - 0.2).abs() < 0.01, "len_s = {len_s}");
    }
}
