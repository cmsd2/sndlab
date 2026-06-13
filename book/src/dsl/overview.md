# DSL overview

The sndlab DSL is a small set of Rhai functions for describing audio
signals. The functions form a tiny **signal-graph DSL**: each call
either constructs a node (`sine`, `noise`) or transforms a node
(`gain`, `env`, `with_taps`). A patch body returns the root node and
the engine renders it to a mono sample buffer.

## Status

| Primitive | Status | Purpose |
|---|---|---|
| [`patch`](./patch.md) | shipped | Register a named patch. |
| [`sine`](./sine.md) | shipped | A sine wave at a given frequency and duration. |
| [`chirp`](./chirp.md) | shipped | Linear-FM sine sweep — broadband source for reverb-style taps. |
| [`noise`](./noise.md) | shipped | White / pink / brown noise. |
| [`env`](./env.md) | shipped | Attack + exponential decay applied to a signal. |
| [`fade_out`](./fade_out.md) | shipped | Cosine-squared fade over the buffer's last `duration_s`. |
| [`tremolo`](./tremolo.md) | shipped | Sine LFO amplitude modulation. |
| [`gain`](./gain.md) | shipped | Linear amplitude scaling. |
| [`bandpass`](./bandpass.md) | shipped | Biquad bandpass — carves resonant peaks from a broadband source. |
| [`lowpass`](./lowpass.md) | shipped | Biquad lowpass — passes content below cutoff, attenuates above. |
| [`highpass`](./highpass.md) | shipped | Biquad highpass — passes content above cutoff, attenuates below. |
| [`mix`](./mix.md) | shipped | Sum multiple signals. |
| [`tap`](./tap.md) | shipped | A delay tap, used by `with_taps`. |
| `with_taps` | shipped | Apply a list of reverb taps to a signal. |

## Conventions

- **Frequencies** are in Hertz. Integers and floats are both accepted
  (e.g. both `440` and `440.0`).
- **Durations** are in seconds (`f64` in the script; cast to `f32` at
  the buffer level).
- **Amplitudes** are linear. `0.0` is silence, `1.0` is full scale.
  Decibel-aware variants are a future convenience.
- **Time-domain parameters** (attack, decay, delay) are seconds.

## Function call style

The DSL is *fluent*: signal-producing functions return a `Signal`
value and transform functions are registered as methods on that value.
This reads naturally:

```rhai
sine(330.0, 3.5).env(0.008, 1.4).gain(0.32)
```

is equivalent to

```rhai
gain(env(sine(330.0, 3.5), 0.008, 1.4), 0.32)
```

— the former is preferred for readability. All transforms also exist
as free functions, but you'll rarely want to use them that way.

## Sample rate

Synthesis runs at a fixed **48 kHz** internally. The audio backend
(Kira via cpal) handles the conversion to the device's preferred rate.
The script never sees samples or rates.

## Determinism

Same script, same buffer. `noise(...)` uses a deterministic PRNG seed
that's stable within a build of the engine; re-evaluating a script
produces an identical buffer. The seed is *not* unique per patch yet
— two noise calls in the same script can correlate. See
[noise](./noise.md) for details.
