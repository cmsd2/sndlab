//! Streaming ambient generation. Patches whose body is a `StreamDef`
//! are not pre-rendered into a buffer at eval time; instead a fresh
//! generator instance is spawned at each play through Kira's custom
//! `Sound` trait, and samples are produced on demand for as long as
//! the handle stays alive. There is no buffer length and no loop
//! boundary, so the loop-wrap class of artefact ceases to exist.
//!
//! The split:
//! - `StreamDef` — pure data, clones cheaply. Stored in the patch
//!   table. Rhai script returns this.
//! - `Stream` (trait) — a *runner* with audio-thread mutable state.
//!   One instance per active playback. Created from a `StreamDef`
//!   via `StreamDef::instantiate()`.
//!
//! All streams run at the engine's fixed 48 kHz sample rate. If
//! Kira plays back at a different device rate the perceived pitch
//! drifts; we accept that trade for the much simpler arithmetic
//! (see `signal::SAMPLE_RATE`).

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use kira::sound::{Sound, SoundData};
use kira::{Frame, Tween};
use rand::Rng;
use rand_pcg::Pcg64;

use crate::signal::{SAMPLE_RATE, NoiseKind};

const DT: f32 = 1.0 / SAMPLE_RATE as f32;
const TWO_PI: f32 = std::f32::consts::TAU;

/// Description of a streaming graph. Cheap to clone — owned by the
/// patch table and re-cloned to produce a fresh runner per play.
///
/// All synthesis primitives in the DSL build a `StreamDef`. A
/// one-shot patch renders this into a finite buffer at registration
/// time (driven by a `Take` or `Chirp` wrapper that supplies a
/// duration); an ambient patch plays it forever through the custom
/// Sound. There is no separate buffer-based pipeline.
#[derive(Clone, Debug)]
pub enum StreamDef {
    Sine {
        freq_hz: f32,
    },
    Chirp {
        start_hz: f32,
        end_hz: f32,
        duration_s: f32,
    },
    Noise {
        kind: NoiseKind,
        seed_a: u64,
        seed_b: u64,
    },
    Gain {
        source: Box<StreamDef>,
        factor: f32,
    },
    Mix(Vec<StreamDef>),
    Biquad {
        source: Box<StreamDef>,
        kind: BiquadKind,
        freq_hz: f32,
        q: f32,
    },
    Env {
        source: Box<StreamDef>,
        attack_s: f32,
        decay_s: f32,
    },
    Tremolo {
        source: Box<StreamDef>,
        rate_hz: f32,
        depth: f32,
    },
    /// Truncate the source to `duration_s`. The runner outputs zeros
    /// after the duration elapses and signals `finished` so Kira can
    /// retire the sound. The presence of a `Take` anywhere in a tree
    /// also tells the one-shot renderer how long to render.
    Take {
        source: Box<StreamDef>,
        duration_s: f32,
    },
    /// Cosine-squared fade-out applied to the source's last
    /// `fade_s` of audible content. Only meaningful when the source
    /// is finite (wrap with `Take` or a similar bounded primitive).
    /// On an unbounded source the fade is a no-op.
    FadeOut {
        source: Box<StreamDef>,
        fade_s: f32,
        /// Captured from the inner `Take` at construction time so
        /// the runner can compute fade start position without
        /// inspecting the graph.
        bounded_to_s: Option<f32>,
    },
    /// sin² fade-in applied to the source's first `fade_s`.
    /// Complementary to `FadeOut`'s cos² — sin² + cos² = 1, so a
    /// fade_in / fade_out pair across two layers reconstructs to
    /// constant power. Works on bounded or unbounded sources.
    FadeIn {
        source: Box<StreamDef>,
        fade_s: f32,
    },
    /// Prepend `delay_s` of silence before the source starts.
    /// Useful for staggering elements in a mix without baking
    /// silence into the source.
    Delay {
        source: Box<StreamDef>,
        delay_s: f32,
    },
    /// Real-time delay-tap reverb. Each tap is a delayed, gain-and-
    /// decay-shaped copy of the source's most recent samples,
    /// summed back into the output.
    WithTaps {
        source: Box<StreamDef>,
        taps: Vec<crate::signal::Tap>,
    },
    /// Stochastic grain generator: brief damped sines fire at a Poisson
    /// rate, each at a random frequency in `[freq_lo_hz, freq_hi_hz]`,
    /// each decaying at `decay_k` per second. Useful for bubble streams,
    /// rain, debris — anything that is many small discrete events at
    /// random times rather than a continuous tone.
    Grains {
        rate_hz: f32,
        freq_lo_hz: f32,
        freq_hi_hz: f32,
        decay_k: f32,
        seed_a: u64,
        seed_b: u64,
    },
    /// A decoded audio sample (MP3/WAV/Ogg/FLAC) played at the engine
    /// sample rate. Linear-interpolated resampling when the source's
    /// native rate differs from 48 kHz. `looping` controls whether
    /// playback restarts at end-of-buffer (ambient-friendly) or
    /// terminates (one-shot-friendly).
    ///
    /// Two independent rate controls:
    /// - `playback_rate` is a tape-speed multiplier — 0.5 plays half-
    ///   speed (octave down, double duration); 2.0 plays double-speed
    ///   (octave up, half duration). Set by `.pitch(...)`. Identity = 1.0.
    /// - `time_stretch` is a pitch-preserving stretch multiplier — 0.5
    ///   plays half-speed at the *same pitch* (using granular
    ///   reconstruction); 2.0 plays twice as fast at the same pitch.
    ///   Set by `.speed(...)`. Identity = 1.0.
    /// They compose: `.pitch(2.0).speed(0.5)` = octave up, original
    /// duration.
    Sample {
        samples: std::sync::Arc<[f32]>,
        source_sr: u32,
        looping: bool,
        playback_rate: f32,
        time_stretch: f32,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum BiquadKind {
    Lowpass,
    Highpass,
    Bandpass,
}

impl StreamDef {
    /// Build a fresh stream runner from this description. Each
    /// playback call creates its own runner so two simultaneous
    /// plays of the same patch don't share oscillator phase or
    /// noise state.
    pub fn instantiate(&self) -> Box<dyn Stream + Send> {
        match self {
            Self::Sine { freq_hz } => Box::new(SineRunner::new(*freq_hz)),
            Self::Chirp {
                start_hz,
                end_hz,
                duration_s,
            } => Box::new(ChirpRunner::new(*start_hz, *end_hz, *duration_s)),
            Self::Noise {
                kind,
                seed_a,
                seed_b,
            } => Box::new(NoiseRunner::new(*kind, *seed_a, *seed_b)),
            Self::Gain { source, factor } => Box::new(GainRunner {
                source: source.instantiate(),
                factor: *factor,
            }),
            Self::Mix(parts) => Box::new(MixRunner {
                sources: parts.iter().map(|p| p.instantiate()).collect(),
                finished: Vec::new(),
            }),
            Self::Biquad {
                source,
                kind,
                freq_hz,
                q,
            } => Box::new(BiquadRunner::new(source.instantiate(), *kind, *freq_hz, *q)),
            Self::Env {
                source,
                attack_s,
                decay_s,
            } => Box::new(EnvRunner::new(source.instantiate(), *attack_s, *decay_s)),
            Self::Tremolo {
                source,
                rate_hz,
                depth,
            } => Box::new(TremoloRunner::new(source.instantiate(), *rate_hz, *depth)),
            Self::Take { source, duration_s } => Box::new(TakeRunner::new(
                source.instantiate(),
                *duration_s,
            )),
            Self::FadeOut {
                source,
                fade_s,
                bounded_to_s,
            } => Box::new(FadeOutRunner::new(
                source.instantiate(),
                *fade_s,
                *bounded_to_s,
            )),
            Self::FadeIn { source, fade_s } => {
                Box::new(FadeInRunner::new(source.instantiate(), *fade_s))
            }
            Self::Delay { source, delay_s } => {
                Box::new(DelayRunner::new(source.instantiate(), *delay_s))
            }
            Self::WithTaps { source, taps } => Box::new(WithTapsRunner::new(
                source.instantiate(),
                taps.clone(),
            )),
            Self::Grains {
                rate_hz,
                freq_lo_hz,
                freq_hi_hz,
                decay_k,
                seed_a,
                seed_b,
            } => Box::new(GrainsRunner::new(
                *rate_hz,
                *freq_lo_hz,
                *freq_hi_hz,
                *decay_k,
                *seed_a,
                *seed_b,
            )),
            Self::Sample {
                samples,
                source_sr,
                looping,
                playback_rate,
                time_stretch,
            } => Box::new(SampleRunner::new(
                samples.clone(),
                *source_sr,
                *looping,
                *playback_rate,
                *time_stretch,
            )),
        }
    }

    /// Walk the graph for a `Take` and return its duration. Used by
    /// the one-shot renderer to decide how many samples to render.
    /// If no `Take` is present, returns `None` — the caller picks a
    /// default cap.
    pub fn finite_duration_s(&self) -> Option<f32> {
        match self {
            Self::Take { duration_s, .. } => Some(*duration_s),
            Self::Chirp { duration_s, .. } => Some(*duration_s),
            Self::FadeOut {
                bounded_to_s: Some(d),
                ..
            } => Some(*d),
            Self::Gain { source, .. }
            | Self::Biquad { source, .. }
            | Self::Env { source, .. }
            | Self::Tremolo { source, .. }
            | Self::FadeOut { source, .. }
            | Self::FadeIn { source, .. }
            | Self::WithTaps { source, .. } => source.finite_duration_s(),
            Self::Delay { source, delay_s } => {
                source.finite_duration_s().map(|d| d + delay_s.max(0.0))
            }
            Self::Mix(parts) => {
                // Mix runs until *every* source has returned None. If
                // any source is unbounded the mix is unbounded too —
                // returning Some(max_of_finite_parts) here would cause
                // a wrapping `fade_out` to terminate the stream at the
                // longest finite layer's end, cutting off the
                // ambient. Only if every part is finite is the mix
                // finite, with duration = the longest of them.
                let mut max_dur: Option<f32> = None;
                for p in parts {
                    match p.finite_duration_s() {
                        Some(d) => {
                            max_dur = Some(max_dur.map_or(d, |m| m.max(d)));
                        }
                        None => return None,
                    }
                }
                max_dur
            }
            Self::Sample {
                samples,
                source_sr,
                looping,
                playback_rate,
                time_stretch,
            } => {
                if *looping {
                    None
                } else {
                    // Duration scales inversely with BOTH rate factors:
                    // base_duration / (pitch_rate × time_stretch).
                    // .pitch(0.5) doubles duration; .speed(0.5) also
                    // doubles duration; both at 0.5 quadruples it.
                    let pitch_rate = playback_rate.max(1e-4);
                    let stretch = time_stretch.max(1e-4);
                    Some(
                        samples.len() as f32
                            / ((*source_sr as f32).max(1.0) * pitch_rate * stretch),
                    )
                }
            }
            Self::Sine { .. } | Self::Noise { .. } | Self::Grains { .. } => None,
        }
    }
}

/// One sample at a time. `tick` returns `Some(sample)` while the
/// stream has audible content; once it returns `None` the stream
/// is considered finished and any further calls also return
/// `None`. Infinite sources (sine, noise, mix of infinite
/// sources, etc.) never return `None`.
pub trait Stream {
    fn tick(&mut self) -> Option<f32>;
}

struct SineRunner {
    phase: f32,
    phase_inc: f32,
}

impl SineRunner {
    fn new(freq_hz: f32) -> Self {
        Self {
            phase: 0.0,
            phase_inc: TWO_PI * freq_hz * DT,
        }
    }
}

impl Stream for SineRunner {
    fn tick(&mut self) -> Option<f32> {
        let s = self.phase.sin();
        self.phase += self.phase_inc;
        if self.phase >= TWO_PI {
            self.phase -= TWO_PI;
        }
        Some(s)
    }
}

struct ChirpRunner {
    start_hz: f32,
    end_hz: f32,
    duration_s: f32,
    samples_total: u64,
    samples_elapsed: u64,
}

impl ChirpRunner {
    fn new(start_hz: f32, end_hz: f32, duration_s: f32) -> Self {
        let samples_total = (duration_s.max(0.0) * SAMPLE_RATE as f32) as u64;
        Self {
            start_hz,
            end_hz,
            duration_s: duration_s.max(0.0),
            samples_total,
            samples_elapsed: 0,
        }
    }
}

impl Stream for ChirpRunner {
    fn tick(&mut self) -> Option<f32> {
        if self.samples_elapsed >= self.samples_total {
            return None;
        }
        let t = self.samples_elapsed as f32 * DT;
        // Linear FM phase formula: φ(t) = 2π · (start·t + 0.5·k·t²)
        let k = if self.duration_s > 0.0 {
            (self.end_hz - self.start_hz) / self.duration_s
        } else {
            0.0
        };
        let phase = TWO_PI * (self.start_hz * t + 0.5 * k * t * t);
        self.samples_elapsed += 1;
        Some(phase.sin())
    }
}

struct NoiseRunner {
    kind: NoiseKind,
    rng: Pcg64,
    // brown integrator state
    acc: f32,
    // pink approximation bands + tick counter
    bands: [f32; 3],
    counter: usize,
}

impl NoiseRunner {
    fn new(kind: NoiseKind, seed_a: u64, seed_b: u64) -> Self {
        Self {
            kind,
            rng: Pcg64::new(seed_a as u128, seed_b as u128),
            acc: 0.0,
            bands: [0.0; 3],
            counter: 0,
        }
    }
}

impl Stream for NoiseRunner {
    fn tick(&mut self) -> Option<f32> {
        let s = match self.kind {
            NoiseKind::White => self.rng.gen_range(-1.0_f32..1.0_f32),
            NoiseKind::Pink => {
                let rates = [1, 4, 16];
                for (b, r) in self.bands.iter_mut().zip(rates.iter()) {
                    if self.counter % r == 0 {
                        *b = self.rng.gen_range(-1.0_f32..1.0_f32);
                    }
                }
                self.counter = self.counter.wrapping_add(1);
                (self.bands[0] + self.bands[1] + self.bands[2]) / 3.0
            }
            NoiseKind::Brown => {
                self.acc = (self.acc + self.rng.gen_range(-0.05_f32..0.05_f32))
                    .clamp(-1.0, 1.0);
                self.acc
            }
        };
        Some(s)
    }
}

struct GainRunner {
    source: Box<dyn Stream + Send>,
    factor: f32,
}

impl Stream for GainRunner {
    fn tick(&mut self) -> Option<f32> {
        self.source.tick().map(|s| s * self.factor)
    }
}

struct MixRunner {
    sources: Vec<Box<dyn Stream + Send>>,
    finished: Vec<bool>,
}

impl MixRunner {
    fn finished_all(&self) -> bool {
        !self.finished.is_empty() && self.finished.iter().all(|f| *f)
    }
}

impl Stream for MixRunner {
    fn tick(&mut self) -> Option<f32> {
        let mut sum = 0.0;
        if self.finished.len() < self.sources.len() {
            self.finished.resize(self.sources.len(), false);
        }
        for (i, s) in self.sources.iter_mut().enumerate() {
            if self.finished[i] {
                continue;
            }
            match s.tick() {
                Some(x) => sum += x,
                None => self.finished[i] = true,
            }
        }
        if self.finished_all() {
            None
        } else {
            Some(sum)
        }
    }
}

struct BiquadRunner {
    source: Box<dyn Stream + Send>,
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl BiquadRunner {
    fn new(
        source: Box<dyn Stream + Send>,
        kind: BiquadKind,
        freq_hz: f32,
        q: f32,
    ) -> Self {
        let q = q.max(1e-3);
        let freq = freq_hz.max(1.0).min(SAMPLE_RATE as f32 * 0.49);
        let w0 = TWO_PI * freq / SAMPLE_RATE as f32;
        let sin_w0 = w0.sin();
        let cos_w0 = w0.cos();
        let alpha = sin_w0 / (2.0 * q);

        let (b0, b1, b2, a0, a1, a2) = match kind {
            BiquadKind::Lowpass => {
                let one_minus_cos = 1.0 - cos_w0;
                (
                    one_minus_cos * 0.5,
                    one_minus_cos,
                    one_minus_cos * 0.5,
                    1.0 + alpha,
                    -2.0 * cos_w0,
                    1.0 - alpha,
                )
            }
            BiquadKind::Highpass => {
                let one_plus_cos = 1.0 + cos_w0;
                (
                    one_plus_cos * 0.5,
                    -one_plus_cos,
                    one_plus_cos * 0.5,
                    1.0 + alpha,
                    -2.0 * cos_w0,
                    1.0 - alpha,
                )
            }
            BiquadKind::Bandpass => (
                alpha,
                0.0,
                -alpha,
                1.0 + alpha,
                -2.0 * cos_w0,
                1.0 - alpha,
            ),
        };
        let inv = 1.0 / a0;
        Self {
            source,
            b0: b0 * inv,
            b1: b1 * inv,
            b2: b2 * inv,
            a1: a1 * inv,
            a2: a2 * inv,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }
}

impl Stream for BiquadRunner {
    fn tick(&mut self) -> Option<f32> {
        let x0 = self.source.tick()?;
        let y0 = self.b0 * x0 + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x0;
        self.y2 = self.y1;
        self.y1 = y0;
        Some(y0)
    }
}

struct EnvRunner {
    source: Box<dyn Stream + Send>,
    attack_s: f32,
    decay_k: f32,
    samples_elapsed: u64,
}

impl EnvRunner {
    fn new(source: Box<dyn Stream + Send>, attack_s: f32, decay_s: f32) -> Self {
        Self {
            source,
            attack_s: attack_s.max(0.0),
            decay_k: decay_s.max(1e-6),
            samples_elapsed: 0,
        }
    }
}

impl Stream for EnvRunner {
    fn tick(&mut self) -> Option<f32> {
        let x = self.source.tick()?;
        let t = self.samples_elapsed as f32 * DT;
        let attack = if self.attack_s > 0.0 {
            (t / self.attack_s).min(1.0)
        } else {
            1.0
        };
        let env = (-t * self.decay_k).exp() * attack;
        self.samples_elapsed += 1;
        Some(x * env)
    }
}

struct TremoloRunner {
    source: Box<dyn Stream + Send>,
    base: f32,
    half_depth: f32,
    two_pi_rate: f32,
    samples_elapsed: u64,
}

impl TremoloRunner {
    fn new(source: Box<dyn Stream + Send>, rate_hz: f32, depth: f32) -> Self {
        let depth = depth.clamp(0.0, 1.0);
        Self {
            source,
            base: 1.0 - depth * 0.5,
            half_depth: depth * 0.5,
            two_pi_rate: TWO_PI * rate_hz,
            samples_elapsed: 0,
        }
    }
}

impl Stream for TremoloRunner {
    fn tick(&mut self) -> Option<f32> {
        let x = self.source.tick()?;
        let t = self.samples_elapsed as f32 * DT;
        let lfo = self.base + self.half_depth * (self.two_pi_rate * t).cos();
        self.samples_elapsed += 1;
        Some(x * lfo)
    }
}

struct TakeRunner {
    source: Box<dyn Stream + Send>,
    samples_remaining: u64,
}

impl TakeRunner {
    fn new(source: Box<dyn Stream + Send>, duration_s: f32) -> Self {
        let samples = (duration_s.max(0.0) * SAMPLE_RATE as f32) as u64;
        Self {
            source,
            samples_remaining: samples,
        }
    }
}

impl Stream for TakeRunner {
    fn tick(&mut self) -> Option<f32> {
        if self.samples_remaining == 0 {
            return None;
        }
        self.samples_remaining -= 1;
        self.source.tick()
    }
}

struct FadeOutRunner {
    source: Box<dyn Stream + Send>,
    fade_samples: u64,
    samples_until_fade: u64,
    samples_into_fade: u64,
    fade_active: bool,
}

impl FadeOutRunner {
    fn new(source: Box<dyn Stream + Send>, fade_s: f32, bounded_to_s: Option<f32>) -> Self {
        let fade_samples = (fade_s.max(0.0) * SAMPLE_RATE as f32) as u64;
        let samples_until_fade = match bounded_to_s {
            Some(total_s) => {
                let total = (total_s.max(0.0) * SAMPLE_RATE as f32) as u64;
                total.saturating_sub(fade_samples)
            }
            // No known length — fade is effectively a no-op.
            None => u64::MAX,
        };
        Self {
            source,
            fade_samples,
            samples_until_fade,
            samples_into_fade: 0,
            fade_active: false,
        }
    }
}

impl Stream for FadeOutRunner {
    fn tick(&mut self) -> Option<f32> {
        let x = self.source.tick()?;
        if self.samples_until_fade > 0 {
            self.samples_until_fade -= 1;
            return Some(x);
        }
        if !self.fade_active {
            self.fade_active = true;
            self.samples_into_fade = 0;
        }
        if self.fade_samples == 0 {
            return Some(x);
        }
        let t = self.samples_into_fade as f32 / self.fade_samples as f32;
        if t >= 1.0 {
            return None;
        }
        let c = (std::f32::consts::FRAC_PI_2 * t).cos();
        let env = c * c;
        self.samples_into_fade += 1;
        Some(x * env)
    }
}

struct FadeInRunner {
    source: Box<dyn Stream + Send>,
    fade_samples: u64,
    samples_elapsed: u64,
}

impl FadeInRunner {
    fn new(source: Box<dyn Stream + Send>, fade_s: f32) -> Self {
        Self {
            source,
            fade_samples: (fade_s.max(0.0) * SAMPLE_RATE as f32) as u64,
            samples_elapsed: 0,
        }
    }
}

impl Stream for FadeInRunner {
    fn tick(&mut self) -> Option<f32> {
        let x = self.source.tick()?;
        if self.fade_samples == 0 || self.samples_elapsed >= self.fade_samples {
            return Some(x);
        }
        let t = self.samples_elapsed as f32 / self.fade_samples as f32;
        // sin² over [0, 1] — complementary to fade_out's cos² so the
        // two energy-sum to a constant.
        let s = (std::f32::consts::FRAC_PI_2 * t).sin();
        let env = s * s;
        self.samples_elapsed += 1;
        Some(x * env)
    }
}

struct DelayRunner {
    source: Box<dyn Stream + Send>,
    silence_samples_remaining: u64,
}

impl DelayRunner {
    fn new(source: Box<dyn Stream + Send>, delay_s: f32) -> Self {
        Self {
            source,
            silence_samples_remaining: (delay_s.max(0.0) * SAMPLE_RATE as f32) as u64,
        }
    }
}

impl Stream for DelayRunner {
    fn tick(&mut self) -> Option<f32> {
        if self.silence_samples_remaining > 0 {
            self.silence_samples_remaining -= 1;
            return Some(0.0);
        }
        self.source.tick()
    }
}

struct WithTapsRunner {
    source: Box<dyn Stream + Send>,
    taps: Vec<TapState>,
    delay_line: Vec<f32>,
    write_pos: usize,
    source_finished: bool,
    inv_sr: f32,
}

struct TapState {
    delay_samples: usize,
    gain: f32,
    decay_k: f32,
}

impl WithTapsRunner {
    fn new(source: Box<dyn Stream + Send>, taps: Vec<crate::signal::Tap>) -> Self {
        let sr = SAMPLE_RATE as f32;
        let max_delay = taps
            .iter()
            .map(|t| (t.delay_s * sr) as usize)
            .max()
            .unwrap_or(0);
        let line_len = max_delay.max(1) + 1;
        let tap_states: Vec<TapState> = taps
            .iter()
            .map(|t| TapState {
                delay_samples: (t.delay_s * sr) as usize,
                gain: t.gain,
                decay_k: t.decay_k,
            })
            .collect();
        Self {
            source,
            taps: tap_states,
            delay_line: vec![0.0; line_len],
            write_pos: 0,
            source_finished: false,
            inv_sr: 1.0 / sr,
        }
    }
}

impl Stream for WithTapsRunner {
    fn tick(&mut self) -> Option<f32> {
        let x = if self.source_finished {
            0.0
        } else {
            match self.source.tick() {
                Some(v) => v,
                None => {
                    self.source_finished = true;
                    0.0
                }
            }
        };

        let line_len = self.delay_line.len();
        self.delay_line[self.write_pos] = x;

        let mut out = x;
        for tap in &self.taps {
            if tap.delay_samples == 0 || tap.delay_samples >= line_len {
                continue;
            }
            // Each tap's envelope from its onset is treated implicitly
            // as constant `gain` here — the decay_k modulation that
            // the buffer impl applies needs more state per tap, so
            // we keep the simpler "delayed copy times gain" for the
            // stream variant for now. Per-tap exponential decay
            // requires a per-tap envelope timer that resets each
            // time a "new attack" arrives, which doesn't directly
            // translate to a real-time delay line.
            let _ = tap.decay_k;
            let read_pos = (self.write_pos + line_len - tap.delay_samples) % line_len;
            out += self.delay_line[read_pos] * tap.gain;
        }

        self.write_pos = (self.write_pos + 1) % line_len;

        // Continue producing the tail until the longest tap delay
        // has decayed off — we'd need explicit time tracking to know
        // when. For now: keep going indefinitely while source is
        // alive, stop a few seconds after source ends.
        // Simplification: stream finishes when source finishes and
        // the delay line has been zero-padded for `line_len`
        // samples (i.e. the tail flushed through).
        if self.source_finished {
            // Heuristic for "delay line is empty": every entry is
            // near zero. Cheap enough at our sizes.
            let still_audible = self.delay_line.iter().any(|s| s.abs() > 1e-5);
            if !still_audible {
                return None;
            }
        }
        let _ = self.inv_sr;
        Some(out)
    }
}

/// Single in-flight grain — phase, frequency, current envelope amplitude.
#[derive(Clone, Copy)]
struct Grain {
    phase: f32,
    phase_inc: f32,
    amp: f32,
}

/// Stochastic grain runner. At each sample we toss a coin weighted by
/// `rate_per_sample` to fire a new grain; each grain is a damped sine
/// at a uniformly-sampled random frequency. Grains are culled when
/// their amplitude falls below the audibility floor, keeping the
/// active set bounded by the rate × mean grain lifetime.
struct GrainsRunner {
    rate_per_sample: f32,
    freq_lo_hz: f32,
    freq_hi_hz: f32,
    per_sample_decay: f32,
    rng: Pcg64,
    grains: Vec<Grain>,
}

impl GrainsRunner {
    const AMP_FLOOR: f32 = 1e-4;
    const MAX_LIVE_GRAINS: usize = 256;

    fn new(
        rate_hz: f32,
        freq_lo_hz: f32,
        freq_hi_hz: f32,
        decay_k: f32,
        seed_a: u64,
        seed_b: u64,
    ) -> Self {
        let rate = rate_hz.max(0.0);
        let decay = decay_k.max(1e-3);
        // Order the frequency bounds so users can pass them either way.
        let (lo, hi) = if freq_lo_hz <= freq_hi_hz {
            (freq_lo_hz, freq_hi_hz)
        } else {
            (freq_hi_hz, freq_lo_hz)
        };
        Self {
            rate_per_sample: rate * DT,
            freq_lo_hz: lo.max(1.0),
            freq_hi_hz: hi.max(lo.max(1.0)),
            per_sample_decay: (-decay * DT).exp(),
            rng: Pcg64::new(seed_a as u128, seed_b as u128),
            grains: Vec::new(),
        }
    }
}

impl Stream for GrainsRunner {
    fn tick(&mut self) -> Option<f32> {
        // Poisson-process approximation: at each sample, fire a new
        // grain with probability `rate × dt`. Good enough below ~kHz
        // rates; above that the per-sample model under-counts because
        // it caps at one grain per sample.
        if self.grains.len() < Self::MAX_LIVE_GRAINS
            && self.rng.gen_range(0.0_f32..1.0_f32) < self.rate_per_sample
        {
            let freq = if self.freq_hi_hz > self.freq_lo_hz {
                self.rng
                    .gen_range(self.freq_lo_hz..self.freq_hi_hz)
            } else {
                self.freq_lo_hz
            };
            self.grains.push(Grain {
                phase: 0.0,
                phase_inc: TWO_PI * freq * DT,
                amp: 1.0,
            });
        }

        let mut sum = 0.0;
        for g in &mut self.grains {
            sum += g.phase.sin() * g.amp;
            g.phase += g.phase_inc;
            if g.phase >= TWO_PI {
                g.phase -= TWO_PI;
            }
            g.amp *= self.per_sample_decay;
        }
        self.grains.retain(|g| g.amp > Self::AMP_FLOOR);

        Some(sum)
    }
}

/// Plays a decoded sample buffer with independent pitch and time-
/// stretch controls.
///
/// Two paths:
/// - **Simple linear-interp** when `time_stretch ≈ 1.0`. Pitch /
///   sample-rate conversion only. Bit-perfect for the no-pitch case,
///   no granular artefacts.
/// - **Granular OLA** when `time_stretch ≠ 1.0`. 4 overlapping Hann-
///   windowed grains at 75% overlap, hopping at `grain_len / 4`. The
///   grain origin advances through the source at the time-stretched
///   rate; within each grain, the read step uses only the pitch
///   factor — so changing speed doesn't change pitch and vice versa.
///   Tiny modulation artefacts on tonal content, but bubbles / noise /
///   textures hide them well.
struct SampleRunner {
    samples: Arc<[f32]>,
    looping: bool,
    finished: bool,

    /// Source samples consumed per output sample for a same-rate copy,
    /// already multiplied by `pitch`. The granular grain-internal read
    /// step uses this directly; the simple path uses it as well.
    src_step_pitched: f64,

    /// True when we need granular time-stretch (time_stretch != 1.0).
    granular: bool,

    // Simple path state.
    simple_pos: f64,

    // Granular path state.
    out_sample_idx: u64,
    grain_len_samples: u64,
    grain_hop_samples: u64,
    /// In source samples, how far the next grain's origin sits past
    /// the previous one's. = (hop_out × src_per_out) / time_stretch.
    grain_origin_advance_src: f64,
    next_grain_origin_src: f64,
    grains: Vec<SampleGrain>,
}

#[derive(Clone, Copy)]
struct SampleGrain {
    start_out: u64,
    origin_src: f64,
}

impl SampleRunner {
    /// Grain length in output samples — 60 ms at 48 kHz.
    const GRAIN_LEN_MS: f64 = 60.0;
    /// Overlap factor: 4 grains active at any time → hop = len/4, 75%
    /// overlap. With Hann windows at this hop, OLA reconstruction sums
    /// to ~2.0 → divide by 2 to normalise.
    const OVERLAP_FACTOR: u64 = 4;
    const OLA_NORM: f32 = 2.0;

    fn new(
        samples: Arc<[f32]>,
        source_sr: u32,
        looping: bool,
        playback_rate: f32,
        time_stretch: f32,
    ) -> Self {
        let source_sr = source_sr.max(1) as f64;
        let pitch = playback_rate.max(1e-4) as f64;
        let stretch = time_stretch.max(1e-4) as f64;

        // Source samples per output sample for an unstretched read,
        // pitched. Slower pitch → smaller step → lower frequency.
        let src_per_out = source_sr / SAMPLE_RATE as f64;
        let src_step_pitched = src_per_out * pitch;

        let granular = (stretch - 1.0).abs() > 1e-6;

        let grain_len_samples =
            (Self::GRAIN_LEN_MS * 1e-3 * SAMPLE_RATE as f64) as u64;
        let grain_hop_samples = (grain_len_samples / Self::OVERLAP_FACTOR).max(1);
        // Each new grain's origin moves forward by hop_out output
        // samples worth of source time, MULTIPLIED by the stretch
        // factor. With stretch < 1 (slower playback / longer
        // duration), the origin advances more slowly, so the source
        // is consumed more slowly — output duration = source /
        // stretch, matching what `finite_duration_s` reports.
        let grain_origin_advance_src = (grain_hop_samples as f64) * src_per_out * stretch;

        Self {
            samples,
            looping,
            finished: false,
            src_step_pitched,
            granular,
            simple_pos: 0.0,
            out_sample_idx: 0,
            grain_len_samples,
            grain_hop_samples,
            grain_origin_advance_src,
            next_grain_origin_src: 0.0,
            grains: Vec::with_capacity(Self::OVERLAP_FACTOR as usize + 1),
        }
    }

    fn read_interp(&self, pos: f64) -> f32 {
        let len = self.samples.len();
        let i = pos.floor() as usize;
        let frac = (pos - pos.floor()) as f32;
        let a = self.samples[i.min(len - 1)];
        let b = self.samples[(i + 1).min(len - 1)];
        a + (b - a) * frac
    }
}

impl Stream for SampleRunner {
    fn tick(&mut self) -> Option<f32> {
        if self.finished || self.samples.is_empty() {
            return None;
        }
        if self.granular {
            self.tick_granular()
        } else {
            self.tick_simple()
        }
    }
}

impl SampleRunner {
    fn tick_simple(&mut self) -> Option<f32> {
        let len = self.samples.len();
        let s = self.read_interp(self.simple_pos);
        self.simple_pos += self.src_step_pitched;
        if self.simple_pos >= len as f64 {
            if self.looping {
                self.simple_pos %= len as f64;
            } else {
                self.finished = true;
            }
        }
        Some(s)
    }

    fn tick_granular(&mut self) -> Option<f32> {
        let len = self.samples.len();
        let len_f = len as f64;

        // Spawn at hop intervals.
        if self.out_sample_idx % self.grain_hop_samples == 0 {
            let in_range = self.looping || self.next_grain_origin_src < len_f;
            if in_range {
                let origin = if self.looping {
                    self.next_grain_origin_src.rem_euclid(len_f)
                } else {
                    self.next_grain_origin_src
                };
                self.grains.push(SampleGrain {
                    start_out: self.out_sample_idx,
                    origin_src: origin,
                });
            }
            self.next_grain_origin_src += self.grain_origin_advance_src;
        }

        let mut sum = 0.0_f32;
        for g in &self.grains {
            let local_t = self.out_sample_idx - g.start_out;
            if local_t >= self.grain_len_samples {
                continue;
            }
            let read_pos_raw = g.origin_src + (local_t as f64) * self.src_step_pitched;
            let read_pos = if self.looping {
                read_pos_raw.rem_euclid(len_f)
            } else if read_pos_raw < 0.0 || read_pos_raw >= (len - 1) as f64 {
                continue;
            } else {
                read_pos_raw
            };
            let s = self.read_interp(read_pos);
            // Hann window across grain lifetime.
            let phase = local_t as f32 / self.grain_len_samples.max(1) as f32;
            let win = 0.5 - 0.5 * (TWO_PI * phase).cos();
            sum += s * win;
        }

        // Cull expired grains.
        let now = self.out_sample_idx;
        let lifetime = self.grain_len_samples;
        self.grains.retain(|g| now - g.start_out < lifetime);

        // Non-looping end condition: no more grains will ever spawn AND
        // every active one has expired.
        if !self.looping
            && self.next_grain_origin_src >= len_f
            && self.grains.is_empty()
        {
            self.finished = true;
        }

        self.out_sample_idx += 1;
        Some(sum / Self::OLA_NORM)
    }
}

// ──────────────────────────────────────────────────────────────
// Kira integration: a custom Sound that runs the stream forever.
// ──────────────────────────────────────────────────────────────

/// Handle returned by `AudioManager::play(StreamingSoundData)`. The
/// engine uses it to stop the stream when the ambient is toggled
/// off or crossfaded out. Cloneable so we can store one copy in
/// the handle table and use another to issue stop commands.
#[derive(Clone)]
pub struct StreamingSoundHandle {
    stop: Arc<AtomicBool>,
    fade_out_ms: Arc<std::sync::atomic::AtomicU64>,
}

impl StreamingSoundHandle {
    pub fn stop(&self, fade_ms: u64) {
        self.fade_out_ms.store(fade_ms, Ordering::Relaxed);
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// SoundData wrapper for a streaming patch. Kira calls
/// `into_sound()` once when `manager.play(data)` is invoked.
pub struct StreamingSoundData {
    pub stream: Box<dyn Stream + Send>,
}

impl SoundData for StreamingSoundData {
    type Error = std::convert::Infallible;
    type Handle = StreamingSoundHandle;

    fn into_sound(self) -> Result<(Box<dyn Sound>, Self::Handle), Self::Error> {
        let stop = Arc::new(AtomicBool::new(false));
        let fade_out_ms = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let sound = StreamingSound {
            stream: self.stream,
            stop: stop.clone(),
            fade_out_ms: fade_out_ms.clone(),
            fade_state: FadeState::Steady,
            fade_samples_remaining: 0,
            fade_total_samples: 0,
        };
        Ok((Box::new(sound), StreamingSoundHandle { stop, fade_out_ms }))
    }
}

enum FadeState {
    Steady,
    FadingOut,
    Done,
}

struct StreamingSound {
    stream: Box<dyn Stream + Send>,
    stop: Arc<AtomicBool>,
    fade_out_ms: Arc<std::sync::atomic::AtomicU64>,
    fade_state: FadeState,
    fade_samples_remaining: u32,
    fade_total_samples: u32,
}

impl Sound for StreamingSound {
    fn process(&mut self, out: &mut [Frame], _dt: f64, _info: &kira::info::Info) {
        // Pick up a stop request lazily — once per buffer is enough
        // since the audio thread comes back here every few ms.
        if matches!(self.fade_state, FadeState::Steady)
            && self.stop.load(Ordering::Relaxed)
        {
            let ms = self.fade_out_ms.load(Ordering::Relaxed);
            if ms == 0 {
                self.fade_state = FadeState::Done;
            } else {
                let total = (ms * SAMPLE_RATE as u64 / 1000) as u32;
                self.fade_total_samples = total.max(1);
                self.fade_samples_remaining = self.fade_total_samples;
                self.fade_state = FadeState::FadingOut;
            }
        }

        for f in out.iter_mut() {
            match self.fade_state {
                FadeState::Done => {
                    *f = Frame::ZERO;
                }
                FadeState::Steady => match self.stream.tick() {
                    Some(s) => *f = Frame::from_mono(s),
                    None => {
                        // Source ran out naturally — one-shot reached
                        // its end via Take/FadeOut/etc.
                        self.fade_state = FadeState::Done;
                        *f = Frame::ZERO;
                    }
                },
                FadeState::FadingOut => {
                    if self.fade_samples_remaining == 0 {
                        self.fade_state = FadeState::Done;
                        *f = Frame::ZERO;
                        continue;
                    }
                    let t = self.fade_samples_remaining as f32
                        / self.fade_total_samples as f32;
                    let env = t;
                    let s = self.stream.tick().unwrap_or(0.0) * env;
                    *f = Frame::from_mono(s);
                    self.fade_samples_remaining -= 1;
                }
            }
        }
    }

    fn finished(&self) -> bool {
        matches!(self.fade_state, FadeState::Done)
    }
}

/// Render the stream into a fixed-length buffer, used by the engine
/// when a one-shot patch needs a finite `Signal`-style buffer to play
/// through Kira's StaticSoundData. The stream is sampled until it
/// returns `None` or `max_samples` is reached, whichever comes first.
pub fn render_to_buffer(mut stream: Box<dyn Stream + Send>, max_samples: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(max_samples);
    for _ in 0..max_samples {
        match stream.tick() {
            Some(s) => out.push(s),
            None => break,
        }
    }
    out
}

// Suppress "Tween / Duration only used in a not-yet-wired path".
fn _silence_unused() {
    let _ = Tween::default();
    let _ = Duration::from_millis(0);
}
