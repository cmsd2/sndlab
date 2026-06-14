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
    /// Real-time delay-tap reverb. Each tap is a delayed, gain-and-
    /// decay-shaped copy of the source's most recent samples,
    /// summed back into the output.
    WithTaps {
        source: Box<StreamDef>,
        taps: Vec<crate::signal::Tap>,
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
            Self::WithTaps { source, taps } => Box::new(WithTapsRunner::new(
                source.instantiate(),
                taps.clone(),
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
            | Self::WithTaps { source, .. } => source.finite_duration_s(),
            Self::Mix(parts) => parts.iter().filter_map(|p| p.finite_duration_s()).fold(
                None,
                |acc, d| match acc {
                    Some(prev) => Some(prev.max(d)),
                    None => Some(d),
                },
            ),
            Self::Sine { .. } | Self::Noise { .. } => None,
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
