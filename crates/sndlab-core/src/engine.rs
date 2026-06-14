//! The Rhai-driven audio engine.
//!
//! `Engine` owns a Rhai interpreter with the patch DSL pre-registered,
//! a shared patch table populated by `patch(...)` calls during
//! evaluation, and (when audio init succeeded) a live Kira audio
//! manager. Evaluating a script replaces the patch table atomically;
//! playing a patch hands its rendered buffer to Kira.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle, StaticSoundSettings};
use kira::{AudioManager, AudioManagerSettings, DefaultBackend, Frame, Tween};

use crate::stream::{StreamDef, StreamingSoundData, StreamingSoundHandle};
use rand_pcg::Pcg64;
use rhai::{Array, Dynamic, ImmutableString, EvalAltResult, Position};

use crate::signal::{self, NoiseKind, Signal, Tap};
use crate::{Buffer, Error, EvalSummary, PatchInfo, PatchRole, Result, SourcePos};

fn convert_position(pos: Position) -> Option<SourcePos> {
    let line = pos.line()?;
    let column = pos.position().unwrap_or(1);
    Some(SourcePos {
        line: line as usize,
        column: column as usize,
    })
}

/// A patch as the engine knows it. Two source types: a pre-rendered
/// buffer (for one-shot patches and for the existing
/// buffer-based ambient path) or a streaming graph (for the new
/// continuously-generated ambient path).
#[derive(Clone)]
struct Patch {
    role: PatchRole,
    source: PatchSource,
}

#[derive(Clone)]
enum PatchSource {
    Buffer {
        samples: Arc<[f32]>,
        sample_rate: u32,
    },
    Stream(StreamDef),
}

/// Shared state mutated by Rhai callbacks during evaluation. Wrapped
/// in `Arc<Mutex<_>>` so Rhai's `Send + Sync` requirement is met and
/// the engine can still read the result after eval returns.
#[derive(Default)]
struct EvalState {
    patches: HashMap<String, Patch>,
    insertion_order: Vec<String>,
    messages: Vec<String>,
}

pub struct Engine {
    rhai: rhai::Engine,
    state: Arc<Mutex<EvalState>>,
    audio: Option<AudioManager<DefaultBackend>>,
    patches: HashMap<String, Patch>,
    patches_order: Vec<String>,
    patches_info: Vec<PatchInfo>,
    /// Active looping ambient playbacks, keyed by patch name.
    /// Cleared on every `eval` so stale handles from re-defined
    /// patches don't outlive their source.
    ambient_handles: HashMap<String, AmbientHandle>,
    /// Directory that relative `sample("…")` paths resolve against.
    /// The host (project model) sets this whenever the active project
    /// changes. Shared by Arc so the Rhai callback can read the
    /// up-to-date value at eval time without a fresh engine rebuild.
    project_root: Arc<Mutex<Option<std::path::PathBuf>>>,
}

/// One live ambient instance. Buffer-based handles control a
/// looped StaticSoundData; stream-based handles control a custom
/// streaming Sound that generates samples on the fly.
enum AmbientHandle {
    Buffer(StaticSoundHandle),
    Stream(StreamingSoundHandle),
}

impl AmbientHandle {
    fn stop_instant(self) {
        match self {
            Self::Buffer(mut h) => h.stop(Tween::default()),
            Self::Stream(h) => h.stop(0),
        }
    }

    fn stop_with_fade(self, fade_ms: u64) {
        match self {
            Self::Buffer(mut h) => h.stop(Tween {
                duration: std::time::Duration::from_millis(fade_ms),
                ..Tween::default()
            }),
            Self::Stream(h) => h.stop(fade_ms),
        }
    }
}

impl Engine {
    pub fn new() -> Result<Self> {
        let state: Arc<Mutex<EvalState>> = Arc::new(Mutex::new(EvalState::default()));
        let project_root = Arc::new(Mutex::new(None));
        let rhai = build_rhai_engine(state.clone(), project_root.clone());
        let audio = open_audio_manager();
        Ok(Self {
            rhai,
            state,
            audio,
            patches: HashMap::new(),
            patches_order: Vec::new(),
            patches_info: Vec::new(),
            ambient_handles: HashMap::new(),
            project_root,
        })
    }

    /// Set the directory relative paths in `sample("…")` resolve
    /// against. Pass `None` to require absolute paths. Takes effect on
    /// the next `eval`.
    pub fn set_project_root(&mut self, root: Option<std::path::PathBuf>) {
        let mut g = self.project_root.lock().expect("project root poisoned");
        *g = root;
    }

    /// Whether the audio backend is alive. If `false`, `play` is a
    /// no-op (with a warning logged); evaluation and patch enumeration
    /// still work.
    pub fn has_audio(&self) -> bool {
        self.audio.is_some()
    }

    /// Evaluate a Rhai script. On success, replaces the engine's
    /// patch table with whatever the script registered. Ambient
    /// handles are intentionally *not* touched here — the caller
    /// decides whether to keep, stop, or crossfade them after seeing
    /// what the new patch table looks like.
    pub fn eval(&mut self, source: &str) -> Result<EvalSummary> {
        // Reset shared state before each eval so a script that doesn't
        // call `patch(...)` ends up with an empty patch table rather
        // than inheriting the previous one.
        {
            let mut s = self.state.lock().expect("eval state poisoned");
            *s = EvalState::default();
        }

        self.rhai
            .run(source)
            .map_err(|e| {
                let position = convert_position(e.position());
                let message = e.to_string();
                match *e {
                    EvalAltResult::ErrorParsing(..) => Error::Parse { message, position },
                    _ => Error::Runtime { message, position },
                }
            })?;

        // Drain into the engine's owned tables.
        let mut s = self.state.lock().expect("eval state poisoned");
        self.patches = std::mem::take(&mut s.patches);
        self.patches_order = std::mem::take(&mut s.insertion_order);
        let messages = std::mem::take(&mut s.messages);
        drop(s);

        self.patches_info = self
            .patches_order
            .iter()
            .filter_map(|name| {
                self.patches.get(name).map(|p| PatchInfo {
                    name: name.clone(),
                    role: p.role,
                    duration_s: match &p.source {
                        PatchSource::Buffer {
                            samples,
                            sample_rate,
                        } => samples.len() as f32 / *sample_rate as f32,
                        // Stream patches are unbounded; report 0.
                        PatchSource::Stream(_) => 0.0,
                    },
                })
            })
            .collect();

        Ok(EvalSummary {
            patches: self.patches_info.clone(),
            messages,
        })
    }

    /// Play an arbitrary buffer through the one-shot path. Used by the
    /// editor's audition feature to play a sliced ambient. Silently
    /// no-ops if the audio backend failed to initialise or the buffer
    /// is empty.
    pub fn play_buffer(&mut self, buffer: &Buffer) -> Result<()> {
        if buffer.samples.is_empty() {
            return Ok(());
        }
        let Some(manager) = self.audio.as_mut() else {
            return Ok(());
        };
        let frames: Vec<Frame> =
            buffer.samples.iter().map(|s| Frame::from_mono(*s)).collect();
        let data = StaticSoundData {
            sample_rate: buffer.sample_rate,
            frames: frames.into(),
            settings: StaticSoundSettings::default(),
            slice: None,
        };
        manager.play(data).map_err(|e| Error::Audio(e.to_string()))?;
        Ok(())
    }

    /// Play a patch by name as a one-shot. Returns an error if the
    /// patch isn't registered. Streaming patches are not playable as
    /// one-shots — they have no end — so this logs a warning and
    /// returns Ok for that case. Silently no-ops if the audio
    /// backend failed to initialise.
    pub fn play(&mut self, name: &str) -> Result<()> {
        let patch = self
            .patches
            .get(name)
            .ok_or_else(|| Error::UnknownPatch(name.into()))?
            .clone();
        let (samples, sample_rate) = match &patch.source {
            PatchSource::Buffer {
                samples,
                sample_rate,
            } => (samples.clone(), *sample_rate),
            PatchSource::Stream(_) => {
                tracing::warn!(
                    "play({name}): patch is streaming-only — use play_ambient instead"
                );
                return Ok(());
            }
        };
        if samples.is_empty() {
            tracing::warn!("play({name}): patch rendered to 0 samples — nothing to play");
            return Ok(());
        }
        let Some(manager) = self.audio.as_mut() else {
            tracing::debug!("play({name}): audio unavailable, dropping");
            return Ok(());
        };
        let frames: Vec<Frame> = samples.iter().map(|s| Frame::from_mono(*s)).collect();
        let data = StaticSoundData {
            sample_rate,
            frames: frames.into(),
            settings: StaticSoundSettings::default(),
            slice: None,
        };
        manager.play(data).map_err(|e| Error::Audio(e.to_string()))?;
        Ok(())
    }

    /// Play a patch as a continuous ambient. Routes to either a
    /// looped buffer (legacy) or a streaming generator depending on
    /// how the patch was registered. The handle is stashed so the
    /// caller can stop it later.
    pub fn play_ambient(&mut self, name: &str) -> Result<()> {
        let patch = self
            .patches
            .get(name)
            .ok_or_else(|| Error::UnknownPatch(name.into()))?
            .clone();
        // Stop whatever's currently playing for this name.
        if let Some(old) = self.ambient_handles.remove(name) {
            old.stop_instant();
        }
        let Some(manager) = self.audio.as_mut() else {
            return Ok(());
        };
        let handle = match &patch.source {
            PatchSource::Buffer {
                samples,
                sample_rate,
            } => {
                if samples.is_empty() {
                    tracing::warn!(
                        "play_ambient({name}): patch rendered to 0 samples — nothing to loop"
                    );
                    return Ok(());
                }
                let frames: Vec<Frame> =
                    samples.iter().map(|s| Frame::from_mono(*s)).collect();
                let data = StaticSoundData {
                    sample_rate: *sample_rate,
                    frames: frames.into(),
                    settings: StaticSoundSettings::default().loop_region(0.0..),
                    slice: None,
                };
                AmbientHandle::Buffer(
                    manager.play(data).map_err(|e| Error::Audio(e.to_string()))?,
                )
            }
            PatchSource::Stream(def) => {
                let data = StreamingSoundData {
                    stream: def.instantiate(),
                };
                AmbientHandle::Stream(
                    manager.play(data).map_err(|e| Error::Audio(e.to_string()))?,
                )
            }
        };
        self.ambient_handles.insert(name.to_string(), handle);
        Ok(())
    }

    /// Stop a specific ambient. No-op if it isn't playing.
    pub fn stop_ambient(&mut self, name: &str) {
        if let Some(h) = self.ambient_handles.remove(name) {
            h.stop_instant();
        }
    }

    /// Stop a specific ambient with a fade-out tween. Drops the
    /// handle immediately — the fade continues in the audio thread
    /// (Kira for buffers, our custom Sound for streams).
    pub fn stop_ambient_with_fade(&mut self, name: &str, fade_ms: u64) {
        if let Some(h) = self.ambient_handles.remove(name) {
            h.stop_with_fade(fade_ms);
        }
    }

    /// Live-coding crossfade: start a fresh instance of the patch,
    /// and fade out the previous instance over `fade_ms`. Works for
    /// both buffer-based and stream-based patches.
    pub fn crossfade_ambient(&mut self, name: &str, fade_ms: u64) -> Result<()> {
        if let Some(old) = self.ambient_handles.remove(name) {
            old.stop_with_fade(fade_ms);
        }
        let patch = self
            .patches
            .get(name)
            .ok_or_else(|| Error::UnknownPatch(name.into()))?
            .clone();
        let Some(manager) = self.audio.as_mut() else {
            return Ok(());
        };
        let handle = match &patch.source {
            PatchSource::Buffer {
                samples,
                sample_rate,
            } => {
                if samples.is_empty() {
                    tracing::warn!(
                        "crossfade_ambient({name}): patch rendered to 0 samples — fading out only"
                    );
                    return Ok(());
                }
                let frames: Vec<Frame> =
                    samples.iter().map(|s| Frame::from_mono(*s)).collect();
                let data = StaticSoundData {
                    sample_rate: *sample_rate,
                    frames: frames.into(),
                    settings: StaticSoundSettings::default().loop_region(0.0..),
                    slice: None,
                };
                AmbientHandle::Buffer(
                    manager.play(data).map_err(|e| Error::Audio(e.to_string()))?,
                )
            }
            PatchSource::Stream(def) => {
                let data = StreamingSoundData {
                    stream: def.instantiate(),
                };
                AmbientHandle::Stream(
                    manager.play(data).map_err(|e| Error::Audio(e.to_string()))?,
                )
            }
        };
        self.ambient_handles.insert(name.to_string(), handle);
        Ok(())
    }

    /// Names of every ambient currently playing. Used by the host
    /// to decide what to crossfade after an eval.
    pub fn ambient_names(&self) -> Vec<String> {
        self.ambient_handles.keys().cloned().collect()
    }

    /// Stop every ambient loop. Called automatically at the start of
    /// each `eval` so stale loops don't outlive their patches.
    pub fn stop_all_ambient(&mut self) {
        for (_, handle) in self.ambient_handles.drain() {
            handle.stop_instant();
        }
    }

    /// Whether a named patch is currently looping as ambient.
    pub fn is_ambient_playing(&self, name: &str) -> bool {
        self.ambient_handles.contains_key(name)
    }

    /// Render a finite slice of a streaming (ambient) patch, for
    /// audition. Spawns a fresh stream runner, ticks `duration_s`
    /// worth of samples, and returns the result as a Buffer. Useful
    /// for live-coding: you keep the patch declared as ambient (it
    /// runs forever in the game) but slice off N seconds in the editor
    /// to play through the scope without re-tuning a separate one-shot
    /// version. Buffer patches (one-shots) also work — they're just
    /// returned via `render`, capped by the slice duration.
    pub fn render_slice(&self, name: &str, duration_s: f32) -> Result<Buffer> {
        let patch = self
            .patches
            .get(name)
            .ok_or_else(|| Error::UnknownPatch(name.into()))?;
        let target_samples =
            (duration_s.max(0.0) * crate::signal::SAMPLE_RATE as f32) as usize;
        match &patch.source {
            PatchSource::Buffer {
                samples,
                sample_rate,
            } => {
                // For one-shots, just truncate at the slice length.
                let take = target_samples.min(samples.len());
                Ok(Buffer {
                    sample_rate: *sample_rate,
                    samples: samples[..take].to_vec().into(),
                })
            }
            PatchSource::Stream(def) => {
                let stream = def.instantiate();
                let samples = crate::stream::render_to_buffer(stream, target_samples);
                Ok(Buffer {
                    sample_rate: crate::signal::SAMPLE_RATE,
                    samples: samples.into(),
                })
            }
        }
    }

    /// Render a patch to a `Buffer` without playing it. Returns an
    /// error for streaming patches — they have no finite buffer.
    pub fn render(&self, name: &str) -> Result<Buffer> {
        let patch = self
            .patches
            .get(name)
            .ok_or_else(|| Error::UnknownPatch(name.into()))?;
        match &patch.source {
            PatchSource::Buffer {
                samples,
                sample_rate,
            } => Ok(Buffer {
                sample_rate: *sample_rate,
                samples: samples.clone(),
            }),
            PatchSource::Stream(_) => Err(Error::Audio(format!(
                "render('{name}'): patch is streaming-only — no finite buffer to render"
            ))),
        }
    }

    pub fn patches(&self) -> &[PatchInfo] {
        &self.patches_info
    }
}

/// Open the audio backend with a sensible buffer size. We start by
/// asking cpal to use a generous fixed buffer (large enough that
/// scheduling jitter on the audio thread doesn't underrun and
/// produce clicks). If the device rejects that — some backends do
/// — we fall back to cpal's default. Either way, init failure is
/// non-fatal: the engine runs silently if no audio can be opened.
fn open_audio_manager() -> Option<AudioManager<DefaultBackend>> {
    use cpal::{
        traits::{DeviceTrait, HostTrait},
        BufferSize, StreamConfig,
    };
    use kira::backend::cpal::CpalBackendSettings;

    // Target: 2048 frames at the device's preferred rate. At 48 kHz
    // that's ~43 ms — comfortably above PipeWire's typical 5 ms
    // quantum, which is the size that's been seen to underrun on
    // moderately loaded Linux desktops.
    const TARGET_FRAMES: u32 = 2048;

    let host = cpal::default_host();
    let device = host.default_output_device();
    let preferred_config = device
        .as_ref()
        .and_then(|d| d.default_output_config().ok());

    let mut backend_settings = CpalBackendSettings::default();
    if let (Some(_), Some(supported)) = (device.as_ref(), preferred_config) {
        let mut stream: StreamConfig = supported.config();
        stream.buffer_size = BufferSize::Fixed(TARGET_FRAMES);
        backend_settings.device = device;
        backend_settings.config = Some(stream);
    }

    let settings_with_buffer = AudioManagerSettings::<DefaultBackend> {
        backend_settings: backend_settings.clone(),
        ..Default::default()
    };

    match AudioManager::<DefaultBackend>::new(settings_with_buffer) {
        Ok(m) => {
            tracing::info!(
                "audio: cpal stream opened with {} frame buffer",
                TARGET_FRAMES
            );
            return Some(m);
        }
        Err(e) => {
            tracing::warn!(
                "audio: cpal rejected a {} frame buffer ({e}); falling back to default",
                TARGET_FRAMES
            );
        }
    }

    match AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()) {
        Ok(m) => Some(m),
        Err(e) => {
            tracing::warn!("audio: init failed, engine running silently: {e}");
            None
        }
    }
}

/// Construct the Rhai engine and register the DSL surface. The
/// `state` `Arc` is captured by the `patch(...)` callback so that
/// patch registrations can be collected into the shared `EvalState`.
fn build_rhai_engine(
    state: Arc<Mutex<EvalState>>,
    project_root: Arc<Mutex<Option<std::path::PathBuf>>>,
) -> rhai::Engine {
    let mut engine = rhai::Engine::new();

    register_unified_dsl(&mut engine);
    register_sample_dsl(&mut engine, project_root);

    // patch() takes a StreamDef. For one-shot patches the engine
    // renders the stream into a finite buffer at eval time; for
    // ambient patches the stream is stored and instantiated per
    // play. Everything in the DSL — sine, noise, env, gain, mix,
    // filters, taps — composes into a StreamDef. There is no
    // separate buffer-based DSL.
    let state_for_patch = state.clone();
    engine.register_fn(
        "patch",
        move |name: ImmutableString, role: ImmutableString, def: StreamDef| -> std::result::Result<(), Box<EvalAltResult>> {
            register_patch(&state_for_patch, name, role, PatchSource::Stream(def))
        },
    );

    // `Dynamic` is used by Rhai internally; we don't expose it here
    // but the import line up top is unused otherwise.
    let _ = Dynamic::UNIT;

    engine
}

fn register_patch(
    state: &Arc<Mutex<EvalState>>,
    name: ImmutableString,
    role: ImmutableString,
    source: PatchSource,
) -> std::result::Result<(), Box<EvalAltResult>> {
    let role = match role.as_str() {
        "one_shot" => PatchRole::OneShot,
        "ambient" => PatchRole::Ambient,
        other => {
            return Err(format!(
                "patch: unknown role '{other}' — expected 'one_shot' or 'ambient'"
            )
            .into());
        }
    };
    // For one-shot patches we render the stream into a finite buffer
    // right now — Kira's low-latency `StaticSoundData` needs the
    // samples up front, and one-shots have an end. The duration is
    // taken from a `Take` / `Chirp` / bounded `FadeOut` in the graph,
    // or a 10-second safety cap if the graph is unbounded.
    let source = match (&role, source) {
        (PatchRole::OneShot, PatchSource::Stream(def)) => {
            const DEFAULT_CAP_S: f32 = 10.0;
            // Hard cap on rendered one-shot duration. Anything longer
            // than this is overwhelmingly likely to be a live-coding
            // typo (`.speed(0.001)` etc.) that would otherwise OOM the
            // process trying to allocate a multi-gigabyte buffer.
            // 10 minutes is well above any genuine sound-design one-
            // shot — raise it if your patch legitimately needs more.
            const HARD_MAX_S: f32 = 600.0;
            let dur = def.finite_duration_s().unwrap_or(DEFAULT_CAP_S);
            if !dur.is_finite() || dur > HARD_MAX_S {
                return Err(format!(
                    "patch '{}' would render to {:.0} s, exceeding the {:.0} s \
                     one-shot cap. Check your `.pitch`/`.speed`/duration values — \
                     a tiny argument (e.g. `.speed(0.001)`) explodes the rendered \
                     length. Make this an ambient patch if you need it unbounded.",
                    name.as_str(),
                    dur,
                    HARD_MAX_S
                )
                .into());
            }
            let max_samples = (dur * crate::signal::SAMPLE_RATE as f32) as usize;
            let stream = def.instantiate();
            let samples = crate::stream::render_to_buffer(stream, max_samples);
            PatchSource::Buffer {
                samples: samples.into(),
                sample_rate: crate::signal::SAMPLE_RATE,
            }
        }
        (_, src) => src,
    };
    let mut s = state.lock().expect("eval state poisoned");
    let name_str = name.to_string();
    if !s.patches.contains_key(&name_str) {
        s.insertion_order.push(name_str.clone());
    } else {
        s.messages.push(format!("patch '{name_str}' redefined"));
    }
    s.patches.insert(name_str, Patch { role, source });
    Ok(())
}

fn register_unified_dsl(engine: &mut rhai::Engine) {
    use crate::stream::BiquadKind;

    engine.register_type_with_name::<StreamDef>("Signal");
    engine.register_type_with_name::<Tap>("Tap");

    // ── Sources ───────────────────────────────────────────────
    // sine(freq) → continuous; sine(freq, dur) → bounded via Take.
    engine.register_fn("sine", |freq_hz: f64| StreamDef::Sine {
        freq_hz: freq_hz as f32,
    });
    engine.register_fn("sine", |freq_hz: i64| StreamDef::Sine {
        freq_hz: freq_hz as f32,
    });
    let sine_take = |freq_hz: f32, duration_s: f32| StreamDef::Take {
        source: Box::new(StreamDef::Sine { freq_hz }),
        duration_s,
    };
    engine.register_fn("sine", move |f: f64, d: f64| sine_take(f as f32, d as f32));
    engine.register_fn("sine", move |f: i64, d: f64| sine_take(f as f32, d as f32));
    engine.register_fn("sine", move |f: f64, d: i64| sine_take(f as f32, d as f32));
    engine.register_fn("sine", move |f: i64, d: i64| sine_take(f as f32, d as f32));

    // chirp(start_hz, end_hz, dur) → bounded LFM sweep.
    let chirp_fn =
        |start: f32, end: f32, dur: f32| StreamDef::Chirp {
            start_hz: start,
            end_hz: end,
            duration_s: dur,
        };
    engine.register_fn("chirp", move |s: f64, e: f64, d: f64| {
        chirp_fn(s as f32, e as f32, d as f32)
    });
    engine.register_fn("chirp", move |s: i64, e: f64, d: f64| {
        chirp_fn(s as f32, e as f32, d as f32)
    });
    engine.register_fn("chirp", move |s: f64, e: i64, d: f64| {
        chirp_fn(s as f32, e as f32, d as f32)
    });
    engine.register_fn("chirp", move |s: i64, e: i64, d: f64| {
        chirp_fn(s as f32, e as f32, d as f32)
    });
    engine.register_fn("chirp", move |s: f64, e: f64, d: i64| {
        chirp_fn(s as f32, e as f32, d as f32)
    });
    engine.register_fn("chirp", move |s: i64, e: i64, d: i64| {
        chirp_fn(s as f32, e as f32, d as f32)
    });

    // noise(kind) → continuous; noise(kind, dur) → bounded.
    fn noise_def(kind: NoiseKind) -> StreamDef {
        // Static seed — deterministic across runs of the same patch.
        let seed_a = 0xcafef00d_d15ea5e5u64;
        let seed_b = kind as u8 as u64 ^ 0xa02b_dbf7_bb3c_0a7;
        StreamDef::Noise {
            kind,
            seed_a,
            seed_b,
        }
    }
    engine.register_fn(
        "noise",
        |kind: ImmutableString| -> std::result::Result<StreamDef, Box<EvalAltResult>> {
            let k = parse_noise_kind(&kind)?;
            Ok(noise_def(k))
        },
    );
    engine.register_fn(
        "noise",
        |kind: ImmutableString,
         duration_s: f64|
         -> std::result::Result<StreamDef, Box<EvalAltResult>> {
            let k = parse_noise_kind(&kind)?;
            Ok(StreamDef::Take {
                source: Box::new(noise_def(k)),
                duration_s: duration_s as f32,
            })
        },
    );

    // grains(rate_hz, freq_lo_hz, freq_hi_hz)              → default decay
    // grains(rate_hz, freq_lo_hz, freq_hi_hz, decay_k)     → explicit decay
    fn grains_def(rate: f32, lo: f32, hi: f32, decay_k: f32) -> StreamDef {
        // Static seeds, varied by frequency range so two grains() calls
        // in the same script don't share state when their parameters
        // happen to match. Deterministic across runs of the same patch.
        let mix = ((lo.to_bits() as u64) << 32) ^ (hi.to_bits() as u64);
        StreamDef::Grains {
            rate_hz: rate,
            freq_lo_hz: lo,
            freq_hi_hz: hi,
            decay_k,
            seed_a: 0xa2cf_7c2f_06a3_42e1 ^ mix,
            seed_b: 0xd1e1_3f6e_92ab_55c3 ^ rate.to_bits() as u64,
        }
    }
    const GRAINS_DEFAULT_DECAY_K: f32 = 80.0; // ~12 ms 1/e — bubble-ish
    macro_rules! reg_grains_3 {
        ($t1:ty, $t2:ty, $t3:ty) => {
            engine.register_fn(
                "grains",
                move |r: $t1, lo: $t2, hi: $t3| {
                    grains_def(r as f32, lo as f32, hi as f32, GRAINS_DEFAULT_DECAY_K)
                },
            );
        };
    }
    macro_rules! reg_grains_4 {
        ($t1:ty, $t2:ty, $t3:ty, $t4:ty) => {
            engine.register_fn(
                "grains",
                move |r: $t1, lo: $t2, hi: $t3, k: $t4| {
                    grains_def(r as f32, lo as f32, hi as f32, k as f32)
                },
            );
        };
    }
    reg_grains_3!(f64, f64, f64);
    reg_grains_3!(i64, f64, f64);
    reg_grains_3!(f64, i64, f64);
    reg_grains_3!(f64, f64, i64);
    reg_grains_3!(i64, i64, f64);
    reg_grains_3!(i64, f64, i64);
    reg_grains_3!(f64, i64, i64);
    reg_grains_3!(i64, i64, i64);
    reg_grains_4!(f64, f64, f64, f64);
    reg_grains_4!(i64, f64, f64, f64);
    reg_grains_4!(i64, i64, i64, f64);
    reg_grains_4!(i64, i64, i64, i64);
    reg_grains_4!(f64, f64, f64, i64);

    // ── Transforms ───────────────────────────────────────────
    let take = |s: StreamDef, d: f32| StreamDef::Take {
        source: Box::new(s),
        duration_s: d,
    };
    engine.register_fn("take", move |s: StreamDef, d: f64| take(s, d as f32));
    engine.register_fn("take", move |s: StreamDef, d: i64| take(s, d as f32));

    let gain = |s: StreamDef, g: f32| StreamDef::Gain {
        source: Box::new(s),
        factor: g,
    };
    engine.register_fn("gain", move |s: StreamDef, g: f64| gain(s, g as f32));
    engine.register_fn("gain", move |s: StreamDef, g: i64| gain(s, g as f32));

    let env = |s: StreamDef, a: f32, d: f32| StreamDef::Env {
        source: Box::new(s),
        attack_s: a,
        decay_s: d,
    };
    engine.register_fn("env", move |s: StreamDef, a: f64, d: f64| {
        env(s, a as f32, d as f32)
    });
    engine.register_fn("env", move |s: StreamDef, a: i64, d: f64| {
        env(s, a as f32, d as f32)
    });
    engine.register_fn("env", move |s: StreamDef, a: f64, d: i64| {
        env(s, a as f32, d as f32)
    });
    engine.register_fn("env", move |s: StreamDef, a: i64, d: i64| {
        env(s, a as f32, d as f32)
    });

    let tremolo = |s: StreamDef, r: f32, depth: f32| StreamDef::Tremolo {
        source: Box::new(s),
        rate_hz: r,
        depth,
    };
    engine.register_fn("tremolo", move |s: StreamDef, r: f64, d: f64| {
        tremolo(s, r as f32, d as f32)
    });
    engine.register_fn("tremolo", move |s: StreamDef, r: i64, d: f64| {
        tremolo(s, r as f32, d as f32)
    });
    engine.register_fn("tremolo", move |s: StreamDef, r: f64, d: i64| {
        tremolo(s, r as f32, d as f32)
    });
    engine.register_fn("tremolo", move |s: StreamDef, r: i64, d: i64| {
        tremolo(s, r as f32, d as f32)
    });

    // pitch(factor) — tape-speed: changes pitch AND duration together.
    // 0.5 = octave down + double duration; 2.0 = octave up + half.
    // Composes by multiplication.
    fn pitch_def(s: StreamDef, factor: f32) -> std::result::Result<StreamDef, Box<EvalAltResult>> {
        match s {
            StreamDef::Sample {
                samples,
                source_sr,
                looping,
                playback_rate,
                time_stretch,
            } => Ok(StreamDef::Sample {
                samples,
                source_sr,
                looping,
                playback_rate: playback_rate * factor,
                time_stretch,
            }),
            _ => Err(
                "pitch(): only valid on a sample — try sample(\"…\").pitch(0.5)".into(),
            ),
        }
    }
    engine.register_fn("pitch", move |s: StreamDef, f: f64| {
        pitch_def(s, f as f32)
    });
    engine.register_fn("pitch", move |s: StreamDef, f: i64| {
        pitch_def(s, f as f32)
    });

    // speed(factor) — pitch-preserving time stretch (granular OLA).
    // 0.5 = half speed at same pitch (double duration); 2.0 = double
    // speed at same pitch (half duration). Composes by multiplication
    // and is independent of `.pitch(...)`.
    fn speed_def(s: StreamDef, factor: f32) -> std::result::Result<StreamDef, Box<EvalAltResult>> {
        match s {
            StreamDef::Sample {
                samples,
                source_sr,
                looping,
                playback_rate,
                time_stretch,
            } => Ok(StreamDef::Sample {
                samples,
                source_sr,
                looping,
                playback_rate,
                time_stretch: time_stretch * factor,
            }),
            _ => Err(
                "speed(): only valid on a sample — try sample(\"…\").speed(0.5)".into(),
            ),
        }
    }
    engine.register_fn("speed", move |s: StreamDef, f: f64| {
        speed_def(s, f as f32)
    });
    engine.register_fn("speed", move |s: StreamDef, f: i64| {
        speed_def(s, f as f32)
    });

    // fade_out captures the source's finite duration at construction
    // time so the runner knows where the fade begins.
    let fade_out = |s: StreamDef, fade_s: f32| {
        let bounded = s.finite_duration_s();
        StreamDef::FadeOut {
            source: Box::new(s),
            fade_s,
            bounded_to_s: bounded,
        }
    };
    engine.register_fn("fade_out", move |s: StreamDef, d: f64| {
        fade_out(s, d as f32)
    });
    engine.register_fn("fade_out", move |s: StreamDef, d: i64| {
        fade_out(s, d as f32)
    });

    // fade_in(seconds): sin² ramp up over the first `fade_s`, then
    // pass through. Complementary to fade_out's cos².
    let fade_in = |s: StreamDef, fade_s: f32| StreamDef::FadeIn {
        source: Box::new(s),
        fade_s,
    };
    engine.register_fn("fade_in", move |s: StreamDef, d: f64| {
        fade_in(s, d as f32)
    });
    engine.register_fn("fade_in", move |s: StreamDef, d: i64| {
        fade_in(s, d as f32)
    });

    // delay(seconds): prepend silence before the source.
    let delay = |s: StreamDef, delay_s: f32| StreamDef::Delay {
        source: Box::new(s),
        delay_s,
    };
    engine.register_fn("delay", move |s: StreamDef, d: f64| {
        delay(s, d as f32)
    });
    engine.register_fn("delay", move |s: StreamDef, d: i64| {
        delay(s, d as f32)
    });

    fn biquad(source: StreamDef, kind: BiquadKind, freq_hz: f32, q: f32) -> StreamDef {
        StreamDef::Biquad {
            source: Box::new(source),
            kind,
            freq_hz,
            q,
        }
    }
    for (name, kind) in [
        ("lowpass", BiquadKind::Lowpass),
        ("highpass", BiquadKind::Highpass),
        ("bandpass", BiquadKind::Bandpass),
    ] {
        engine.register_fn(name, move |s: StreamDef, f: f64, q: f64| {
            biquad(s, kind, f as f32, q as f32)
        });
        engine.register_fn(name, move |s: StreamDef, f: i64, q: f64| {
            biquad(s, kind, f as f32, q as f32)
        });
        engine.register_fn(name, move |s: StreamDef, f: f64, q: i64| {
            biquad(s, kind, f as f32, q as f32)
        });
        engine.register_fn(name, move |s: StreamDef, f: i64, q: i64| {
            biquad(s, kind, f as f32, q as f32)
        });
    }

    // ── Mix ──────────────────────────────────────────────────
    engine.register_fn(
        "mix",
        |sigs: Array| -> std::result::Result<StreamDef, Box<EvalAltResult>> {
            let mut converted = Vec::with_capacity(sigs.len());
            for (i, s) in sigs.into_iter().enumerate() {
                match s.try_cast::<StreamDef>() {
                    Some(s) => converted.push(s),
                    None => return Err(format!("mix: element {i} is not a Signal").into()),
                }
            }
            Ok(StreamDef::Mix(converted))
        },
    );

    // ── Taps ─────────────────────────────────────────────────
    engine.register_fn("tap", |delay_s: f64, gain: f64| {
        Tap::new(delay_s as f32, gain as f32, Tap::DEFAULT_DECAY_K)
    });
    engine.register_fn("tap", |delay_s: f64, gain: f64, decay_k: f64| {
        Tap::new(delay_s as f32, gain as f32, decay_k as f32)
    });
    engine.register_fn("tap", |delay_s: f64, gain: f64, decay_k: i64| {
        Tap::new(delay_s as f32, gain as f32, decay_k as f32)
    });
    engine.register_fn(
        "with_taps",
        |s: StreamDef, taps: Array| -> std::result::Result<StreamDef, Box<EvalAltResult>> {
            let mut converted = Vec::with_capacity(taps.len());
            for (i, t) in taps.into_iter().enumerate() {
                match t.try_cast::<Tap>() {
                    Some(t) => converted.push(t),
                    None => return Err(format!("with_taps: element {i} is not a Tap").into()),
                }
            }
            Ok(StreamDef::WithTaps {
                source: Box::new(s),
                taps: converted,
            })
        },
    );
}

fn register_sample_dsl(
    engine: &mut rhai::Engine,
    project_root: Arc<Mutex<Option<std::path::PathBuf>>>,
) {
    fn resolve(
        project_root: &Arc<Mutex<Option<std::path::PathBuf>>>,
        path: &str,
    ) -> std::result::Result<std::path::PathBuf, Box<EvalAltResult>> {
        let p = std::path::PathBuf::from(path);
        if p.is_absolute() {
            return Ok(p);
        }
        let guard = project_root.lock().expect("project root poisoned");
        match guard.as_ref() {
            Some(root) => Ok(root.join(p)),
            None => Err(format!(
                "sample('{path}'): relative path but no project root is set — save the project first, or use an absolute path"
            )
            .into()),
        }
    }

    fn load(
        project_root: &Arc<Mutex<Option<std::path::PathBuf>>>,
        path: ImmutableString,
        looping: bool,
    ) -> std::result::Result<StreamDef, Box<EvalAltResult>> {
        let resolved = resolve(project_root, path.as_str())?;
        let buffer = crate::decode::decode_file(&resolved).map_err(|e| -> Box<EvalAltResult> {
            format!(
                "sample('{}'): {e}",
                resolved.display()
            )
            .into()
        })?;
        Ok(StreamDef::Sample {
            samples: buffer.samples,
            source_sr: buffer.sample_rate,
            looping,
            playback_rate: 1.0,
            time_stretch: 1.0,
        })
    }

    let pr_oneshot = project_root.clone();
    engine.register_fn(
        "sample",
        move |path: ImmutableString| -> std::result::Result<StreamDef, Box<EvalAltResult>> {
            load(&pr_oneshot, path, false)
        },
    );

    let pr_loop = project_root;
    engine.register_fn(
        "sample_loop",
        move |path: ImmutableString| -> std::result::Result<StreamDef, Box<EvalAltResult>> {
            load(&pr_loop, path, true)
        },
    );
}

fn parse_noise_kind(s: &str) -> std::result::Result<NoiseKind, Box<EvalAltResult>> {
    match s {
        "white" => Ok(NoiseKind::White),
        "pink" => Ok(NoiseKind::Pink),
        "brown" => Ok(NoiseKind::Brown),
        other => Err(format!(
            "noise: unknown kind '{other}' — expected 'white', 'pink', or 'brown'"
        )
        .into()),
    }
}
