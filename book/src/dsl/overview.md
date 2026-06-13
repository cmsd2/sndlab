# DSL overview

The sndlab DSL is a small set of Rhai functions for describing audio
signals. The functions form a tiny **signal-graph DSL**: each call
either constructs a node (`sine`, `noise`) or transforms a node
(`gain`, `env`, `with_taps`). A patch body returns the root node and
the engine renders it.

> The DSL is being added incrementally. The table below tracks status
> as primitives ship. Each row links to a chapter with the full
> signature, semantics, and examples.

## Status

| Primitive | Status | Purpose |
|---|---|---|
| [`patch`](./patch.md) | not yet implemented | Register a named patch. |
| [`sine`](./sine.md) | not yet implemented | A sine wave at a given frequency and duration. |
| [`noise`](./noise.md) | not yet implemented | White / pink / brown noise. |
| [`env`](./env.md) | not yet implemented | Attack/decay envelope applied to a signal. |
| [`gain`](./gain.md) | not yet implemented | Linear amplitude scaling. |
| [`mix`](./mix.md) | not yet implemented | Sum multiple signals. |
| [`tap`](./tap.md) | not yet implemented | A delay tap (used for reverb tails). |

## Conventions

- **Frequencies** are in Hertz (`f32`).
- **Durations** are in seconds (`f32`).
- **Amplitudes** are linear (`0.0`–`1.0`); decibels are a future
  convenience.
- **Time-domain parameters** (attack, decay, delay) are seconds.

## Function call style

The DSL is *fluent*: signal-producing functions return a "signal
handle" and transform functions are called as methods on that handle.
This reads naturally:

```rhai
sine(330.0, 3.5).env(0.008, 1.4).gain(0.32)
```

is the same as

```rhai
gain(env(sine(330.0, 3.5), 0.008, 1.4), 0.32)
```

— the former is preferred for readability.
