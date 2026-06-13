# `tap`

A single delay tap, used with `with_taps` to build discrete reflection-style
reverb tails.

## Signature

```rhai
tap(delay_s, gain) -> Tap          // default decay (~80 ms 1/e)
tap(delay_s, gain, decay_k) -> Tap // explicit decay

signal.with_taps([tap_a, tap_b, ...]) -> Signal
```

A `Tap` is a `(delay_s, gain, decay_k)` triple. `with_taps` copies the source
into the buffer at each tap's offset, scaled by `gain`, then applies an
independent exponential envelope per tap shaped by `decay_k`. The output
extends past the source by the longest tap so the tail has somewhere to live.

## What `decay_k` does

`decay_k` is the per-sample exponential decay rate measured against the tap's
own onset. The tap envelope at time `t` after onset is `exp(-t × decay_k)`.

| `decay_k` | 1/e time | Sounds like |
|---|---|---|
| `12` (default) | ~83 ms | A discrete reflection — a brief echo of the source's onset. |
| `6` | ~167 ms | A longer late reflection. |
| `3` | ~333 ms | A diffused tail element. |
| `0` | (no decay) | A literal delayed copy of the entire source. Useful for chord-stacking, not for reverb. |

If you want sustained delayed copies of the source — the old behaviour — pass
`tap(delay, gain, 0)`. If you want reflections, the two-argument form does
the right thing.

## Why discrete decays, not convolution reverb

A real reverb convolves the source with the room's impulse response. That's
expensive (FFT-domain or 5000+ taps) and overkill for a game-shaped reverb
tail. Six decaying taps render in microseconds and give the "near reflections
then late tail" character listeners expect.

A proper convolution reverb will land later when the recipes need it
(probably for cathedral / open-ocean sounds). For sonar-room geometry,
discrete reflection-style taps are the right tool.

## Example

```rhai
patch("sonar_ping", "one_shot",
    sine(330.0, 1.5).env(0.008, 1.4).gain(0.32)
        .with_taps([
            tap(0.13, 0.7),    // close, loud, brief
            tap(0.31, 0.5),    // mid, slightly quieter
            tap(0.58, 0.35),   // farther, quieter still
            tap(0.95, 0.22),   // distant late reflection
        ]));
```

That's the submarine ping: a sine body with four reflections, each later
and quieter, each itself brief (default 80 ms decay).

## Errors

- `with_taps: element N is not a Tap` — one of the array elements wasn't built
  with `tap(...)`.

## Notes

- **Older patches** that relied on sustained-copy taps (the only behaviour
  before this change) need to add `, 0` to their `tap(...)` calls to keep
  sounding the same.
- **Combining decays**: a single signal can have many taps with different
  `decay_k` values — short close reflections plus a long diffused tail.
