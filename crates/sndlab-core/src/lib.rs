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

    /// A tap with a fast decay produces less energy at its tail than
    /// at its onset — the discriminator between "reflection tap" and
    /// "sustained copy". A tap with `decay_k = 0` is the legacy
    /// sustained-copy behaviour. To isolate the tap from the dry
    /// signal, the source is shorter than the tap delay.
    #[test]
    fn tap_decay_shapes_the_tail() {
        let mut engine = Engine::new().expect("engine init");
        // Source: 0.4 s. Tap delay: 0.5 s. So the tap plays from
        // 0.5–0.9 s with the dry already finished, and any energy
        // we measure inside the tap window is the tap alone.
        engine
            .eval(
                r#"
                patch("fast", "one_shot",
                    sine(220.0, 0.4).gain(0.5)
                        .with_taps([tap(0.5, 1.0)]));
            "#,
            )
            .expect("eval ok");
        let fast = engine.render("fast").expect("render ok");
        // 50 ms window at tap onset vs 50 ms window 0.3 s into the
        // tap. With decay_k = 12 the second window is tiny.
        let onset_sample = (0.5 * fast.sample_rate as f32) as usize;
        let early =
            window_rms(&fast.samples, onset_sample, onset_sample + 2_400);
        let late = window_rms(
            &fast.samples,
            onset_sample + 14_400,
            onset_sample + 16_800,
        );
        assert!(
            early > 10.0 * late,
            "fast-decay tap should attenuate sharply: early={early}, late={late}"
        );

        // decay_k = 0 → sustained copy. The same 0.3-s-in window
        // should be roughly the source's RMS, not near-zero.
        engine
            .eval(
                r#"
                patch("sustained", "one_shot",
                    sine(220.0, 0.4).gain(0.5)
                        .with_taps([tap(0.5, 1.0, 0.0)]));
            "#,
            )
            .expect("eval ok");
        let sustained = engine.render("sustained").expect("render ok");
        let late_sustained = window_rms(
            &sustained.samples,
            onset_sample + 14_400,
            onset_sample + 16_800,
        );
        assert!(
            late_sustained > 10.0 * late,
            "sustained tap should not attenuate: sustained late={late_sustained}, fast late={late}"
        );
    }

    fn window_rms(samples: &[f32], start: usize, end: usize) -> f32 {
        let end = end.min(samples.len());
        if end <= start {
            return 0.0;
        }
        let sum_sq: f32 = samples[start..end].iter().map(|s| s * s).sum();
        (sum_sq / (end - start) as f32).sqrt()
    }

    /// A chirp degenerates to a sine when start_hz == end_hz. The
    /// generated buffers should be sample-equivalent.
    #[test]
    fn chirp_with_constant_freq_equals_sine() {
        let mut engine = Engine::new().expect("engine init");
        engine
            .eval(
                r#"
                patch("c", "one_shot", chirp(440.0, 440.0, 0.5));
                patch("s", "one_shot", sine(440.0, 0.5));
            "#,
            )
            .expect("eval ok");
        let c = engine.render("c").expect("render c");
        let s = engine.render("s").expect("render s");
        assert_eq!(c.samples.len(), s.samples.len());
        let max_err = c
            .samples
            .iter()
            .zip(s.samples.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        assert!(max_err < 1e-3, "chirp/sine match: max_err = {max_err}");
    }

    /// An upward chirp has more zero-crossings per unit time in the
    /// late portion than the early portion — the operational
    /// definition of "frequency rises over the buffer."
    #[test]
    fn chirp_frequency_increases_over_time() {
        let mut engine = Engine::new().expect("engine init");
        engine
            .eval(
                r#"
                patch("sweep", "one_shot", chirp(200.0, 800.0, 1.0));
            "#,
            )
            .expect("eval ok");
        let buf = engine.render("sweep").expect("render ok");
        let n = buf.samples.len();
        let early = zero_crossings(&buf.samples[0..n / 8]);
        let late = zero_crossings(&buf.samples[7 * n / 8..n]);
        assert!(
            late > 3 * early,
            "late should have ~4× the zero crossings of early (200 → 800 Hz): early={early}, late={late}"
        );
    }

    fn zero_crossings(samples: &[f32]) -> usize {
        samples
            .windows(2)
            .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
            .count()
    }

    /// fade_out leaves the body of the buffer unchanged and brings
    /// the last region smoothly down to zero. The very last sample
    /// must be at or below the noise floor of the cosine ramp.
    #[test]
    fn fade_out_smooths_the_tail() {
        let mut engine = Engine::new().expect("engine init");
        engine
            .eval(
                r#"
                patch("p", "one_shot", sine(440.0, 1.0).fade_out(0.3));
            "#,
            )
            .expect("eval ok");
        let buf = engine.render("p").expect("render ok");
        let n = buf.samples.len();
        let body = window_rms(&buf.samples, 0, n.saturating_sub(15_000));
        let tail = window_rms(&buf.samples, n - 2_400, n);
        assert!(
            body > 10.0 * tail,
            "tail should be far quieter than the body: body={body}, tail={tail}"
        );
        // Final sample must be essentially silent.
        let last = buf.samples.last().copied().unwrap_or(0.0).abs();
        assert!(last < 0.01, "last sample should be ~0: {last}");
    }

    /// A lowpass at 5 kHz passes a 1 kHz sine intact and attenuates a
    /// 10 kHz sine heavily. Inverse for highpass.
    #[test]
    fn lowpass_and_highpass_separate_bands() {
        let mut engine = Engine::new().expect("engine init");
        engine
            .eval(
                r#"
                patch("lp_pass", "one_shot", sine(1000.0, 0.5).lowpass(5000.0, 0.707));
                patch("lp_cut",  "one_shot", sine(10000.0, 0.5).lowpass(5000.0, 0.707));
                patch("hp_pass", "one_shot", sine(10000.0, 0.5).highpass(5000.0, 0.707));
                patch("hp_cut",  "one_shot", sine(1000.0, 0.5).highpass(5000.0, 0.707));
            "#,
            )
            .expect("eval ok");
        let skip = 4_800;
        let lp_pass = engine.render("lp_pass").unwrap();
        let lp_cut = engine.render("lp_cut").unwrap();
        let hp_pass = engine.render("hp_pass").unwrap();
        let hp_cut = engine.render("hp_cut").unwrap();
        let lp_pass_rms = window_rms(&lp_pass.samples, skip, lp_pass.samples.len());
        let lp_cut_rms = window_rms(&lp_cut.samples, skip, lp_cut.samples.len());
        let hp_pass_rms = window_rms(&hp_pass.samples, skip, hp_pass.samples.len());
        let hp_cut_rms = window_rms(&hp_cut.samples, skip, hp_cut.samples.len());
        assert!(
            lp_pass_rms > 5.0 * lp_cut_rms,
            "lowpass at 5k: 1k should pass, 10k should cut. pass={lp_pass_rms}, cut={lp_cut_rms}"
        );
        assert!(
            hp_pass_rms > 5.0 * hp_cut_rms,
            "highpass at 5k: 10k should pass, 1k should cut. pass={hp_pass_rms}, cut={hp_cut_rms}"
        );
    }

    /// A bandpass at the source's centre frequency passes the signal
    /// (after a brief biquad transient); the same filter applied to a
    /// sine well outside the passband attenuates dramatically.
    #[test]
    fn bandpass_passes_centre_attenuates_far() {
        let mut engine = Engine::new().expect("engine init");
        engine
            .eval(
                r#"
                patch("pass", "one_shot", sine(1000.0, 1.0).bandpass(1000.0, 10.0));
                patch("stop", "one_shot", sine(5000.0, 1.0).bandpass(1000.0, 10.0));
            "#,
            )
            .expect("eval ok");
        let pass = engine.render("pass").expect("render ok");
        let stop = engine.render("stop").expect("render ok");
        // Skip the first 100 ms to avoid the biquad's transient ramp.
        let skip = 4_800;
        let pass_rms = window_rms(&pass.samples, skip, pass.samples.len());
        let stop_rms = window_rms(&stop.samples, skip, stop.samples.len());
        assert!(
            pass_rms > 20.0 * stop_rms,
            "Q=10 at 1 kHz should kill 5 kHz: pass_rms={pass_rms}, stop_rms={stop_rms}"
        );
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
