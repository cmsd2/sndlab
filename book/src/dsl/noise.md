# `noise`

A noise generator. Three flavours.

## Signature

```rhai
noise(kind: string)               -> Signal   // unbounded; runs forever in an ambient
noise(kind: string, duration_s)   -> Signal   // bounded
```

- **`kind`** — one of:
  - `"white"` — uniform random in `[-1, 1)`, flat spectrum.
  - `"pink"` — pseudo-pink via a three-band integrator approximation.
    Close enough to true pink for ambient layers; not for measurement.
  - `"brown"` — integrated white, clamped to `±1`. Heavy low-frequency
    content; useful for ocean rumble.
- **`duration_s`** — length of the resulting buffer, in seconds.

## Example

```rhai
let hiss   = noise("white", 0.5).gain(0.1);
let ambient = noise("pink", 8.0).gain(0.3);
let rumble  = noise("brown", 12.0).gain(0.6);
```

## Errors

- `noise: unknown kind 'foo' — expected 'white', 'pink', or 'brown'`

## Determinism caveat

The current implementation uses a fixed PRNG seed shared across all
`noise(...)` calls in an evaluation. Two calls in the same script
share state, which means re-evaluating produces identical buffers
but the *relative* sequences between two noise calls in the same
script are not independent.

This is good enough for game ambience but worth knowing. A future
change will seed per-`patch(...)` so noise within different patches
is statistically independent, and a `seed: int` parameter will let
the caller pin it explicitly.
