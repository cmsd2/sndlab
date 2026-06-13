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

    /// Constant-skirt-gain biquad bandpass (RBJ Audio EQ Cookbook).
    /// Carves a resonant peak at `center_hz` from the source. `q`
    /// controls the peak's width: bandwidth ≈ `center_hz / q`. Useful
    /// values run roughly 0.5 (very broad) to 50 (very narrow).
    ///
    /// The transient response of the biquad means the first few
    /// milliseconds of the output ramp up from zero; usually masked
    /// by the envelope you apply.
    pub fn bandpass(&self, center_hz: f32, q: f32) -> Signal {
        let (cos_w0, alpha) = biquad_w0_alpha(center_hz, q);
        let b0 = alpha;
        let b1 = 0.0;
        let b2 = -alpha;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;
        Signal::new(apply_biquad(&self.samples, b0, b1, b2, a0, a1, a2))
    }

    /// Biquad lowpass (RBJ Audio EQ Cookbook). Passes content below
    /// `cutoff_hz`, attenuates above. `q` controls the resonance at
    /// the cutoff knee — `0.707` is the standard Butterworth value
    /// (no resonant peak). Higher Q (~2–10) creates an audible
    /// emphasis right at the cutoff.
    pub fn lowpass(&self, cutoff_hz: f32, q: f32) -> Signal {
        let (cos_w0, alpha) = biquad_w0_alpha(cutoff_hz, q);
        let one_minus_cos = 1.0 - cos_w0;
        let b0 = one_minus_cos * 0.5;
        let b1 = one_minus_cos;
        let b2 = one_minus_cos * 0.5;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;
        Signal::new(apply_biquad(&self.samples, b0, b1, b2, a0, a1, a2))
    }

    /// Tremolo — amplitude modulation by a low-frequency sine. The
    /// modulator runs `cos(2π·rate_hz·t)`, scaled so that `depth = 0`
    /// is a no-op and `depth = 1` swings the amplitude between 0 and
    /// the input's full level. Common musical values: `rate_hz` in
    /// 3–8 Hz, `depth` in 0.3–0.7. Slower rates (< 1 Hz) feel like
    /// volume swelling; faster (> 15 Hz) cross into rough AM
    /// territory.
    ///
    /// Composes naturally with `env`: `signal.env(...).tremolo(...)`
    /// gives an exponential decay with a wobble laid on top.
    pub fn tremolo(&self, rate_hz: f32, depth: f32) -> Signal {
        let depth = depth.clamp(0.0, 1.0);
        let half_depth = depth * 0.5;
        let base = 1.0 - half_depth;
        let two_pi_rate = std::f32::consts::TAU * rate_hz;
        let inv_sr = 1.0 / SAMPLE_RATE_F;
        let mut out = Vec::with_capacity(self.samples.len());
        for (i, &s) in self.samples.iter().enumerate() {
            let t = i as f32 * inv_sr;
            // cos starts at 1, so the modulator begins at full
            // amplitude — no immediate dip at the signal's onset.
            let lfo = base + half_depth * (two_pi_rate * t).cos();
            out.push(s * lfo);
        }
        Signal::new(out)
    }

    /// Smooth cosine-squared fade-out applied to the **last**
    /// `duration_s` of the buffer. Composes naturally with `env` —
    /// the envelope shapes the body, the fade catches whatever's
    /// left at the end so the buffer terminates at silence
    /// regardless of how loud the body still was. Use this when a
    /// single exponential `env` would leave audible level at the
    /// buffer boundary and produce a click on playback.
    ///
    /// The shape is `cos²(π·t/2)` over the fade region, smooth at
    /// both ends — no derivative discontinuity where the fade starts
    /// or where it reaches zero.
    pub fn fade_out(&self, duration_s: f32) -> Signal {
        let n = self.samples.len();
        if n == 0 || duration_s <= 0.0 {
            return self.clone();
        }
        let fade_samples = ((duration_s * SAMPLE_RATE_F) as usize).min(n).max(1);
        let fade_start = n - fade_samples;
        let inv_len = 1.0 / fade_samples as f32;
        let mut out = Vec::with_capacity(n);
        for (i, &s) in self.samples.iter().enumerate() {
            let gain = if i < fade_start {
                1.0
            } else {
                let t = (i - fade_start) as f32 * inv_len;
                let c = (std::f32::consts::FRAC_PI_2 * t).cos();
                c * c
            };
            out.push(s * gain);
        }
        Signal::new(out)
    }

    /// Biquad highpass (RBJ Audio EQ Cookbook). Mirror of `lowpass`:
    /// passes content above `cutoff_hz`, attenuates below. Same `q`
    /// intuition — 0.707 is Butterworth (flat), higher resonates.
    pub fn highpass(&self, cutoff_hz: f32, q: f32) -> Signal {
        let (cos_w0, alpha) = biquad_w0_alpha(cutoff_hz, q);
        let one_plus_cos = 1.0 + cos_w0;
        let b0 = one_plus_cos * 0.5;
        let b1 = -one_plus_cos;
        let b2 = one_plus_cos * 0.5;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;
        Signal::new(apply_biquad(&self.samples, b0, b1, b2, a0, a1, a2))
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

    /// Reflection-style tail: at each tap's offset, sum a delayed,
    /// attenuated, *fast-decaying* copy of the source into the buffer.
    /// The per-tap exponential decay (`tap.decay_k`) is what makes
    /// each tap sound like a brief reflection rather than a sustained
    /// replay of the entire source. With `decay_k = 0` the tap is a
    /// literal delayed copy; with `decay_k = 12.0` (the default for
    /// two-arg `tap`) it falls to 1/e in ~80 ms, which sounds like a
    /// discrete reflection of the source's attack transient.
    ///
    /// The output extends past the source by the longest tap delay so
    /// the tail of the last tap has somewhere to live.
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
        let inv_sr = 1.0 / SAMPLE_RATE_F;
        for tap in taps {
            let offset = (tap.delay_s * SAMPLE_RATE_F) as usize;
            for (i, &v) in self.samples.iter().enumerate() {
                let dst = offset + i;
                if dst >= n {
                    break;
                }
                // `t` is measured from the tap's onset, not the source's
                // — so each tap has its own envelope timer.
                let t = i as f32 * inv_sr;
                let env = if tap.decay_k > 0.0 {
                    (-t * tap.decay_k).exp()
                } else {
                    1.0
                };
                out[dst] += v * tap.gain * env;
            }
        }
        Signal::new(out)
    }
}

/// One element of a reverb-tail tap list. Carries a delay, a gain,
/// and a per-tap exponential decay rate. `decay_k = 0` means the tap
/// is a literal delayed copy of the source (the old behaviour, useful
/// for stacking sustained chord-like tones). Positive values shape the
/// tap into a brief reflection.
#[derive(Clone, Copy)]
pub struct Tap {
    pub delay_s: f32,
    pub gain: f32,
    pub decay_k: f32,
}

impl Tap {
    /// Construct a tap with an explicit decay rate.
    pub fn new(delay_s: f32, gain: f32, decay_k: f32) -> Self {
        Self {
            delay_s,
            gain,
            decay_k,
        }
    }

    /// Default reflection decay (1/e at ~80 ms). Used when the script
    /// calls the two-argument form of `tap`.
    pub const DEFAULT_DECAY_K: f32 = 12.0;
}

/// Compute the (cos w0, α) pair shared by all RBJ biquad shapes.
/// Clamps freq to a safe sub-Nyquist range and Q to a positive value.
fn biquad_w0_alpha(freq_hz: f32, q: f32) -> (f32, f32) {
    let q = q.max(1e-3);
    let freq = freq_hz.max(1.0).min(SAMPLE_RATE_F * 0.49);
    let w0 = std::f32::consts::TAU * freq / SAMPLE_RATE_F;
    let alpha = w0.sin() / (2.0 * q);
    (w0.cos(), alpha)
}

/// Run a Direct-Form-I biquad with the given un-normalised
/// coefficients across `samples`. The caller supplies a non-zero
/// `a0`; this function normalises by it and runs the recurrence.
fn apply_biquad(
    samples: &[f32],
    b0: f32,
    b1: f32,
    b2: f32,
    a0: f32,
    a1: f32,
    a2: f32,
) -> Vec<f32> {
    let inv_a0 = 1.0 / a0;
    let b0 = b0 * inv_a0;
    let b1 = b1 * inv_a0;
    let b2 = b2 * inv_a0;
    let a1 = a1 * inv_a0;
    let a2 = a2 * inv_a0;

    let mut x1 = 0.0_f32;
    let mut x2 = 0.0_f32;
    let mut y1 = 0.0_f32;
    let mut y2 = 0.0_f32;
    let mut out = Vec::with_capacity(samples.len());
    for &x0 in samples {
        let y0 = b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
        x2 = x1;
        x1 = x0;
        y2 = y1;
        y1 = y0;
        out.push(y0);
    }
    out
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

/// Build a linear-frequency-modulated chirp (LFM) sweeping from
/// `start_hz` to `end_hz` over `duration_s` seconds. Amplitude is
/// unit; apply `.gain()` to scale.
///
/// The instantaneous frequency varies linearly with time:
///   f(t) = start_hz + (end_hz - start_hz) * (t / duration_s)
/// The phase is the integral:
///   φ(t) = 2π · (start_hz · t + 0.5 · k · t²)   where k = (end-start)/dur
///
/// Why this exists: a pure sine is monochromatic, so summed delayed
/// copies (taps) lock into a stable comb-filter pattern with
/// audible sustained nulls. A chirp is broadband across the sweep,
/// so the delay-induced interference moves through frequencies and
/// the result sounds like reverb rather than a phaser. The sonar
/// ping seed patch uses this for exactly that reason.
pub fn chirp(start_hz: f32, end_hz: f32, duration_s: f32) -> Signal {
    let n = (duration_s * SAMPLE_RATE_F).max(0.0) as usize;
    let mut out = Vec::with_capacity(n);
    let inv_sr = 1.0 / SAMPLE_RATE_F;
    let k = if duration_s > 0.0 {
        (end_hz - start_hz) / duration_s
    } else {
        0.0
    };
    let tau = std::f32::consts::TAU;
    for i in 0..n {
        let t = i as f32 * inv_sr;
        let phase = tau * (start_hz * t + 0.5 * k * t * t);
        out.push(phase.sin());
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

