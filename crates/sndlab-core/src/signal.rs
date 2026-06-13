//! Signal and Tap types — the values the DSL traffics in.
//!
//! Signals are eagerly rendered into a mono sample buffer at the
//! engine's fixed sample rate. Every primitive (`sine`, `noise`)
//! allocates a fresh buffer; every transform (`env`, `gain`,
//! `with_taps`) returns a new buffer. This is wasteful in absolute
//! terms but irrelevant in practice — patches are short (seconds, not
//! minutes) and we render them at script-evaluation time, not at
//! audio-rate.

use std::sync::Arc;

use rand::Rng;

/// Engine-wide synthesis sample rate. Kira / cpal handle device-rate
/// resampling when the patch plays, so the script can pretend the
/// world runs at one fixed rate.
pub const SAMPLE_RATE: u32 = 48_000;
const SAMPLE_RATE_F: f32 = SAMPLE_RATE as f32;

/// A mono audio buffer. Cheaply cloneable — the samples are shared via
/// `Arc`. Transforms return a fresh `Signal` with new samples.
#[derive(Clone)]
pub struct Signal {
    pub sample_rate: u32,
    pub samples: Arc<[f32]>,
}

impl Signal {
    pub fn new(samples: Vec<f32>) -> Self {
        Self {
            sample_rate: SAMPLE_RATE,
            samples: samples.into(),
        }
    }

    /// Number of samples in the buffer.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Duration in seconds.
    pub fn duration_s(&self) -> f32 {
        self.samples.len() as f32 / self.sample_rate as f32
    }

    /// Apply a fast-attack-then-exponential-decay envelope in place
    /// of the source. Returns a fresh buffer (the source is shared via
    /// `Arc` and may be referenced elsewhere).
    pub fn env(&self, attack_s: f32, decay_s: f32) -> Signal {
        let n = self.samples.len();
        let mut out = Vec::with_capacity(n);
        let inv_sr = 1.0 / SAMPLE_RATE_F;
        // Decay constant: an `decay_s` of 1.0 means the envelope falls
        // by 1/e in one second. Smaller values = longer ring; larger =
        // faster die-off. Matches the audio-design.md intuition.
        let decay_k = decay_s.max(1e-6);
        let attack_s = attack_s.max(0.0);
        for (i, s) in self.samples.iter().enumerate() {
            let t = i as f32 * inv_sr;
            let attack = if attack_s > 0.0 {
                (t / attack_s).min(1.0)
            } else {
                1.0
            };
            let env = (-t * decay_k).exp() * attack;
            out.push(s * env);
        }
        Signal::new(out)
    }

    /// Linear amplitude scaling. `factor = 0.5` halves; `2.0` doubles.
    pub fn gain(&self, factor: f32) -> Signal {
        let out: Vec<f32> = self.samples.iter().map(|s| s * factor).collect();
        Signal::new(out)
    }

    /// Sum a slice of signals. All signals are zero-padded to the
    /// longest's length; the output length is `max(len(s) for s in signals)`.
    pub fn mix(signals: &[Signal]) -> Signal {
        if signals.is_empty() {
            return Signal::new(Vec::new());
        }
        let n = signals.iter().map(|s| s.samples.len()).max().unwrap_or(0);
        let mut out = vec![0.0_f32; n];
        for s in signals {
            for (i, &v) in s.samples.iter().enumerate() {
                out[i] += v;
            }
        }
        Signal::new(out)
    }

    /// Reverb-style tail: copy the source at each tap's offset with the
    /// tap's gain, summed back into the buffer. The output extends past
    /// the source by the longest tap if needed.
    pub fn with_taps(&self, taps: &[Tap]) -> Signal {
        let src_len = self.samples.len();
        let extra = taps
            .iter()
            .map(|t| (t.delay_s * SAMPLE_RATE_F) as usize)
            .max()
            .unwrap_or(0);
        let n = src_len + extra;
        let mut out = vec![0.0_f32; n];
        for (i, &v) in self.samples.iter().enumerate() {
            out[i] = v;
        }
        for tap in taps {
            let offset = (tap.delay_s * SAMPLE_RATE_F) as usize;
            for (i, &v) in self.samples.iter().enumerate() {
                let dst = offset + i;
                if dst < n {
                    out[dst] += v * tap.gain;
                }
            }
        }
        Signal::new(out)
    }
}

/// One element of a reverb-tail tap list. Carries a delay and a gain
/// relative to the source.
#[derive(Clone, Copy)]
pub struct Tap {
    pub delay_s: f32,
    pub gain: f32,
}

impl Tap {
    pub fn new(delay_s: f32, gain: f32) -> Self {
        Self { delay_s, gain }
    }
}

/// Build a sine wave at `freq_hz`, lasting `duration_s` seconds, with
/// unit amplitude. Apply `.gain()` to scale.
pub fn sine(freq_hz: f32, duration_s: f32) -> Signal {
    let n = (duration_s * SAMPLE_RATE_F).max(0.0) as usize;
    let mut out = Vec::with_capacity(n);
    let inv_sr = 1.0 / SAMPLE_RATE_F;
    let two_pi_f = std::f32::consts::TAU * freq_hz;
    for i in 0..n {
        let t = i as f32 * inv_sr;
        out.push((two_pi_f * t).sin());
    }
    Signal::new(out)
}

/// What kind of noise to generate. Pink and brown are derived from
/// white via simple integrators; not exactly textbook-pink but close
/// enough for game ambience.
#[derive(Clone, Copy)]
pub enum NoiseKind {
    White,
    Pink,
    Brown,
}

/// Deterministic per-patch noise. The PCG generator should already be
/// seeded by the caller from a stable source (e.g. the patch name's
/// hash) so re-evaluating produces identical samples.
pub fn noise(kind: NoiseKind, duration_s: f32, rng: &mut rand_pcg::Pcg64) -> Signal {
    let n = (duration_s * SAMPLE_RATE_F).max(0.0) as usize;
    let mut out = Vec::with_capacity(n);
    match kind {
        NoiseKind::White => {
            for _ in 0..n {
                out.push(rng.gen_range(-1.0_f32..1.0_f32));
            }
        }
        NoiseKind::Pink => {
            // 3-band Voss-style approximation: sum three integrators
            // updating at different rates. Cheap, sounds pink enough.
            let mut bands = [0.0_f32; 3];
            let rates = [1, 4, 16];
            for i in 0..n {
                for (b, r) in bands.iter_mut().zip(rates.iter()) {
                    if i % r == 0 {
                        *b = rng.gen_range(-1.0_f32..1.0_f32);
                    }
                }
                let s = (bands[0] + bands[1] + bands[2]) / 3.0;
                out.push(s);
            }
        }
        NoiseKind::Brown => {
            let mut acc = 0.0_f32;
            for _ in 0..n {
                acc = (acc + rng.gen_range(-0.05_f32..0.05_f32)).clamp(-1.0, 1.0);
                out.push(acc);
            }
        }
    }
    Signal::new(out)
}

