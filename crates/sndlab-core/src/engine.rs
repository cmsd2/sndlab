//! The Rhai-driven audio engine.
//!
//! `Engine` owns a Rhai interpreter with the patch DSL pre-registered,
//! a shared patch table populated by `patch(...)` calls during
//! evaluation, and (when audio init succeeded) a live Kira audio
//! manager. Evaluating a script replaces the patch table atomically;
//! playing a patch hands its rendered buffer to Kira.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use kira::sound::static_sound::{StaticSoundData, StaticSoundSettings};
use kira::{AudioManager, AudioManagerSettings, DefaultBackend, Frame};
use rand_pcg::Pcg64;
use rhai::{Array, Dynamic, ImmutableString, EvalAltResult};

use crate::signal::{self, NoiseKind, Signal, Tap};
use crate::{Buffer, Error, EvalSummary, PatchInfo, PatchRole, Result};

/// A patch as the engine knows it: the rendered buffer plus its role.
#[derive(Clone)]
struct Patch {
    role: PatchRole,
    samples: Arc<[f32]>,
    sample_rate: u32,
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
}

impl Engine {
    pub fn new() -> Result<Self> {
        let state: Arc<Mutex<EvalState>> = Arc::new(Mutex::new(EvalState::default()));
        let rhai = build_rhai_engine(state.clone());
        let audio = match AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()) {
            Ok(m) => Some(m),
            Err(e) => {
                tracing::warn!("audio: init failed, engine running silently: {e}");
                None
            }
        };
        Ok(Self {
            rhai,
            state,
            audio,
            patches: HashMap::new(),
            patches_order: Vec::new(),
            patches_info: Vec::new(),
        })
    }

    /// Whether the audio backend is alive. If `false`, `play` is a
    /// no-op (with a warning logged); evaluation and patch enumeration
    /// still work.
    pub fn has_audio(&self) -> bool {
        self.audio.is_some()
    }

    /// Evaluate a Rhai script. On success, replaces the engine's
    /// patch table with whatever the script registered.
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
            .map_err(|e| match *e {
                EvalAltResult::ErrorParsing(..) => Error::Parse(e.to_string()),
                _ => Error::Runtime(e.to_string()),
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
                    duration_s: p.samples.len() as f32 / p.sample_rate as f32,
                })
            })
            .collect();

        Ok(EvalSummary {
            patches: self.patches_info.clone(),
            messages,
        })
    }

    /// Play a patch by name. Returns an error if the patch isn't
    /// registered. Silently no-ops if the audio backend failed to
    /// initialise; the caller should check `has_audio()` if it cares.
    pub fn play(&mut self, name: &str) -> Result<()> {
        let patch = self
            .patches
            .get(name)
            .ok_or_else(|| Error::UnknownPatch(name.into()))?
            .clone();
        let Some(manager) = self.audio.as_mut() else {
            tracing::debug!("play({name}): audio unavailable, dropping");
            return Ok(());
        };
        let frames: Vec<Frame> = patch.samples.iter().map(|s| Frame::from_mono(*s)).collect();
        let data = StaticSoundData {
            sample_rate: patch.sample_rate,
            frames: frames.into(),
            settings: StaticSoundSettings::default(),
            slice: None,
        };
        manager.play(data).map_err(|e| Error::Audio(e.to_string()))?;
        Ok(())
    }

    /// Render a patch to a `Buffer` without playing it. Useful for
    /// testing and offline rendering.
    pub fn render(&self, name: &str) -> Result<Buffer> {
        let patch = self
            .patches
            .get(name)
            .ok_or_else(|| Error::UnknownPatch(name.into()))?;
        Ok(Buffer {
            sample_rate: patch.sample_rate,
            samples: patch.samples.clone(),
        })
    }

    pub fn patches(&self) -> &[PatchInfo] {
        &self.patches_info
    }
}

/// Construct the Rhai engine and register the DSL surface. The
/// `state` `Arc` is captured by the `patch(...)` callback so that
/// patch registrations can be collected into the shared `EvalState`.
fn build_rhai_engine(state: Arc<Mutex<EvalState>>) -> rhai::Engine {
    let mut engine = rhai::Engine::new();

    // Custom types: Rhai needs to know about them to allow method-call
    // syntax (`sig.env(...)`) and array elements (`[tap(...), tap(...)]`).
    engine.register_type_with_name::<Signal>("Signal");
    engine.register_type_with_name::<Tap>("Tap");

    // Source primitives.
    engine.register_fn("sine", |freq_hz: f64, duration_s: f64| {
        signal::sine(freq_hz as f32, duration_s as f32)
    });
    engine.register_fn("sine", |freq_hz: i64, duration_s: f64| {
        signal::sine(freq_hz as f32, duration_s as f32)
    });
    engine.register_fn("sine", |freq_hz: f64, duration_s: i64| {
        signal::sine(freq_hz as f32, duration_s as f32)
    });
    engine.register_fn("sine", |freq_hz: i64, duration_s: i64| {
        signal::sine(freq_hz as f32, duration_s as f32)
    });

    // Linear FM chirp. Rhai needs every numeric type combination
    // spelled out individually; lambdas keep the boilerplate tight.
    let chirp_fn = |start: f32, end: f32, dur: f32| signal::chirp(start, end, dur);
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

    engine.register_fn(
        "noise",
        |kind: ImmutableString, duration_s: f64| -> std::result::Result<Signal, Box<EvalAltResult>> {
            let k = match kind.as_str() {
                "white" => NoiseKind::White,
                "pink" => NoiseKind::Pink,
                "brown" => NoiseKind::Brown,
                other => {
                    return Err(format!(
                        "noise: unknown kind '{other}' — expected 'white', 'pink', or 'brown'"
                    )
                    .into());
                }
            };
            // Noise gets a fresh PRNG seeded from a stable salt. The
            // caller doesn't need to think about determinism — same
            // script, same buffer.
            let mut rng = Pcg64::new(0xcafef00d_d15ea5e5, 0xa02bdbf7bb3c0a7);
            Ok(signal::noise(k, duration_s as f32, &mut rng))
        },
    );

    // Transforms — registered as both standalone functions and as
    // methods on Signal so the fluent style works.
    engine.register_fn("env", |s: Signal, attack_s: f64, decay_s: f64| {
        s.env(attack_s as f32, decay_s as f32)
    });
    engine.register_fn("gain", |s: Signal, factor: f64| s.gain(factor as f32));
    engine.register_fn("gain", |s: Signal, factor: i64| s.gain(factor as f32));

    // Cosine-squared fade applied to the buffer's last `duration_s`.
    engine.register_fn("fade_out", |s: Signal, duration_s: f64| {
        s.fade_out(duration_s as f32)
    });
    engine.register_fn("fade_out", |s: Signal, duration_s: i64| {
        s.fade_out(duration_s as f32)
    });

    // Biquad bandpass — applies a resonant peak filter to the source.
    // Rhai needs every numeric type combination spelled out.
    engine.register_fn("bandpass", |s: Signal, center: f64, q: f64| {
        s.bandpass(center as f32, q as f32)
    });
    engine.register_fn("bandpass", |s: Signal, center: i64, q: f64| {
        s.bandpass(center as f32, q as f32)
    });
    engine.register_fn("bandpass", |s: Signal, center: f64, q: i64| {
        s.bandpass(center as f32, q as f32)
    });
    engine.register_fn("bandpass", |s: Signal, center: i64, q: i64| {
        s.bandpass(center as f32, q as f32)
    });

    // Biquad lowpass / highpass. q ≈ 0.707 is Butterworth (flat
    // passband); higher q creates resonance at the cutoff knee.
    engine.register_fn("lowpass", |s: Signal, cutoff: f64, q: f64| {
        s.lowpass(cutoff as f32, q as f32)
    });
    engine.register_fn("lowpass", |s: Signal, cutoff: i64, q: f64| {
        s.lowpass(cutoff as f32, q as f32)
    });
    engine.register_fn("lowpass", |s: Signal, cutoff: f64, q: i64| {
        s.lowpass(cutoff as f32, q as f32)
    });
    engine.register_fn("lowpass", |s: Signal, cutoff: i64, q: i64| {
        s.lowpass(cutoff as f32, q as f32)
    });

    engine.register_fn("highpass", |s: Signal, cutoff: f64, q: f64| {
        s.highpass(cutoff as f32, q as f32)
    });
    engine.register_fn("highpass", |s: Signal, cutoff: i64, q: f64| {
        s.highpass(cutoff as f32, q as f32)
    });
    engine.register_fn("highpass", |s: Signal, cutoff: f64, q: i64| {
        s.highpass(cutoff as f32, q as f32)
    });
    engine.register_fn("highpass", |s: Signal, cutoff: i64, q: i64| {
        s.highpass(cutoff as f32, q as f32)
    });

    // Tap constructors. The two-arg form picks a sensible default
    // decay so the tap sounds like a brief reflection; the three-arg
    // form is explicit. `decay_k = 0` opts back into the legacy
    // "sustained delayed copy" semantics.
    engine.register_fn("tap", |delay_s: f64, gain: f64| {
        Tap::new(delay_s as f32, gain as f32, Tap::DEFAULT_DECAY_K)
    });
    engine.register_fn("tap", |delay_s: f64, gain: f64, decay_k: f64| {
        Tap::new(delay_s as f32, gain as f32, decay_k as f32)
    });
    engine.register_fn("tap", |delay_s: f64, gain: f64, decay_k: i64| {
        Tap::new(delay_s as f32, gain as f32, decay_k as f32)
    });

    // Reverb-tap application.
    engine.register_fn(
        "with_taps",
        |s: Signal, taps: Array| -> std::result::Result<Signal, Box<EvalAltResult>> {
            let mut converted = Vec::with_capacity(taps.len());
            for (i, t) in taps.into_iter().enumerate() {
                match t.try_cast::<Tap>() {
                    Some(t) => converted.push(t),
                    None => return Err(format!("with_taps: element {i} is not a Tap").into()),
                }
            }
            Ok(s.with_taps(&converted))
        },
    );

    // Mix takes an array of Signals and sums them.
    engine.register_fn(
        "mix",
        |sigs: Array| -> std::result::Result<Signal, Box<EvalAltResult>> {
            let mut converted = Vec::with_capacity(sigs.len());
            for (i, s) in sigs.into_iter().enumerate() {
                match s.try_cast::<Signal>() {
                    Some(s) => converted.push(s),
                    None => return Err(format!("mix: element {i} is not a Signal").into()),
                }
            }
            Ok(Signal::mix(&converted))
        },
    );

    // The patch registration entry point. Captures `state` so we can
    // collect patches from inside the Rhai callback.
    let state_for_patch = state.clone();
    engine.register_fn(
        "patch",
        move |name: ImmutableString, role: ImmutableString, signal: Signal| -> std::result::Result<(), Box<EvalAltResult>> {
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
            let mut s = state_for_patch.lock().expect("eval state poisoned");
            let name_str = name.to_string();
            if !s.patches.contains_key(&name_str) {
                s.insertion_order.push(name_str.clone());
            } else {
                s.messages.push(format!("patch '{name_str}' redefined"));
            }
            s.patches.insert(
                name_str,
                Patch {
                    role,
                    samples: signal.samples,
                    sample_rate: signal.sample_rate,
                },
            );
            Ok(())
        },
    );

    // `Dynamic` is used by Rhai internally; we don't expose it here
    // but the import line up top is unused otherwise.
    let _ = Dynamic::UNIT;

    engine
}
