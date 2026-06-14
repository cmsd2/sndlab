# DSL overview

The sndlab DSL is a small set of Rhai functions for describing audio
signals. Every primitive — source, transform, or combinator — builds
a node in a single `Signal` graph. The graph is **lazy**: nothing is
computed until the engine decides what to do with it.

What the engine does depends on the patch's role:

- **One-shot patches** render the graph into a finite buffer at
  patch-registration time, driven by whatever bounded duration the
  graph supplies (a `take(...)`, a `chirp(...)`, or a sound
  primitive called with a duration argument). The buffer plays
  through Kira's low-latency `StaticSoundData` path.
- **Ambient patches** keep the graph as a lazy description. At play
  time a fresh runner is spawned and ticks the graph at audio rate
  for as long as the ambient stays enabled. There is no looping —
  ambients are *generated*, not played back.

The single-graph design means there's no distinction between
"buffer DSL" and "stream DSL." `sine(440)` is the same primitive
whether it ends up in a one-shot or an ambient — only the duration
context differs.

## Status

| Primitive | Status | Purpose |
|---|---|---|
| [`patch`](./patch.md) | shipped | Register a named patch. |
| [`sine`](./sine.md) | shipped | A sine oscillator. `sine(freq)` is unbounded; `sine(freq, dur)` wraps in `take`. |
| [`chirp`](./chirp.md) | shipped | Linear-FM sweep, bounded by its `duration` argument. |
| [`noise`](./noise.md) | shipped | `noise(kind)` is unbounded; `noise(kind, dur)` wraps in `take`. |
| [`env`](./env.md) | shipped | Attack + exponential decay applied to a signal. |
| `take` | shipped | Truncate a signal to `duration_s`. Sources support this implicitly via their two-arg form. |
| [`fade_out`](./fade_out.md) | shipped | Cosine-squared fade over the buffer's last `duration_s`. Requires a bounded source. |
| [`tremolo`](./tremolo.md) | shipped | Sine LFO amplitude modulation. |
| [`gain`](./gain.md) | shipped | Linear amplitude scaling. |
| [`bandpass`](./bandpass.md) | shipped | Biquad bandpass. |
| [`lowpass`](./lowpass.md) | shipped | Biquad lowpass. |
| [`highpass`](./highpass.md) | shipped | Biquad highpass. |
| [`mix`](./mix.md) | shipped | Sum multiple signals. |
| [`tap`](./tap.md) | shipped | A delay tap, used by `with_taps`. (Per-tap exponential decay is honoured in one-shots only; ambient streams use fixed-gain delay copies.) |
| [`grains`](./grains.md) | shipped | Stochastic damped-sine grain generator — bubbles, drips, rain. |
| [`sample`](./sample.md) | shipped | Load an audio file (MP3/WAV/Ogg/FLAC) as a Signal. `sample(path)` plays once; `sample_loop(path)` wraps. |

## Conventions

- **Frequencies** in Hz. Integers and floats both accepted.
- **Durations** in seconds.
- **Amplitudes** linear, 0..1.

## Fluent style

```rhai
sine(330.0).env(0.008, 1.4).gain(0.32).take(3.5)
```

Equivalent to nesting calls — the engine's Rhai layer just registers
each function as both a free fn and a method on `Signal`.

## Bounding rules

One-shot patches need a finite duration somewhere in the graph. The
engine walks the tree and uses the longest `take` / `chirp` /
bounded `fade_out` it finds; if nothing's bounded it caps at a 10 s
safety default and logs a warning. Ambient patches ignore bounded
sub-trees and simply run forever.
