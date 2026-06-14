# `grains`

A stochastic grain generator. Brief damped sines fire at a Poisson
rate, each at a random frequency in a band. Useful for textures made
of many small discrete events — bubble streams, rain on a hull,
sand pouring, debris.

## Signature

```rhai
grains(rate_hz, freq_lo_hz, freq_hi_hz)              -> Signal   // default decay
grains(rate_hz, freq_lo_hz, freq_hi_hz, decay_k)     -> Signal   // explicit decay
```

- **`rate_hz`** — expected number of grain onsets per second. At each
  audio sample we toss a weighted coin: probability `rate_hz / 48000`
  of firing a new grain. 20–80 sounds like effervescence; 1–5 like a
  slow drip; 200+ blends into a tonal hash.
- **`freq_lo_hz`** / **`freq_hi_hz`** — frequency range. Each grain
  picks uniformly within. The bounds can be passed in either order;
  they're normalised internally.
- **`decay_k`** — per-second exponential decay rate of each grain's
  amplitude envelope (same units as `tap`'s `decay_k`). The default is
  `80` (≈12 ms 1/e — short, popping bubble-like). Drop to `20` (~50 ms)
  for longer rings; raise to `200` for sharp ticks.

The output is unbounded — runs forever in an ambient. Wrap with `take`
or `fade_out` if you want a finite version.

## Why this models bubbles

A real bubble in water has a resonant frequency determined by its
radius (the [Minnaert resonance](https://en.wikipedia.org/wiki/Minnaert_resonance):
~3 kHz·m / radius). Small bubbles ring high (1–3 kHz); larger ones
ring low. A bubble cloud is a swarm of such resonances overlapping at
random onsets. Synthesising that as a sum of randomly-triggered
damped sines is closer to the physics than filtered noise — and much
closer to "I hear bubbles" than a continuous bandpass-shaped hiss.

For a torpedo wake or hull-vent texture:

```rhai
patch("bubble_stream", "ambient",
    grains(60.0, 600.0, 2800.0)             // 60 bubbles/s, small/medium
        .lowpass(4000.0, 0.707)             // tame the top end
        .gain(0.35));
```

For sparse drips:

```rhai
patch("hull_drip", "ambient",
    grains(2.0, 800.0, 1500.0, 30.0)        // 2 drips/s, longer ring
        .gain(0.4));
```

## Parameters at a glance

| Effect you want | `rate_hz` | `decay_k` | `freq_lo`–`freq_hi` |
|---|---|---|---|
| Fizzy effervescence | 80–200 | 80 | 1500–4000 |
| Coarse bubbling (torpedo wake) | 40–80 | 60 | 500–2500 |
| Big slow blobs | 5–10 | 30 | 100–800 |
| Sharp rain ticks | 20–60 | 200 | 3000–8000 |
| Sand / debris hash | 200–500 | 150 | 2000–6000 |

## Composing with other primitives

`grains` returns a normal `Signal`, so the full chain is available:

- `.lowpass(…)` / `.bandpass(…)` — colour the grain band as a whole
- `.gain(…)` — set level
- `.tremolo(rate, depth)` — add a long-period swell, e.g. wake rising and falling
- `.with_taps([…])` — feed into reverb taps for a wet bubbly tail

Layer it inside a `mix([…])` alongside `noise(…)` for the
continuous-wash component and `sine(…)` for any tonal element of the
texture (motor hum under bubbles, etc.).

## Notes

- **Determinism.** Like `noise`, grains use a fixed PRNG seed derived
  from the call's parameters. Two `grains` calls with identical
  parameters in one script will share the same sequence; vary one
  parameter slightly (e.g. `grains(60.0, 600.0, 2800.0)` vs
  `grains(60.0, 605.0, 2800.0)`) for independent textures.
- **Backpressure.** The runner caps concurrent live grains at 256.
  Above that the rate effectively saturates. With the default decay
  this corresponds to ~3 kHz onset rate before clipping kicks in —
  beyond any sound design need.
- **CPU.** Each live grain is one phase-step + one sine + one mul.
  At 100 Hz onset rate with `decay_k=80`, mean concurrency is ~6
  grains. Negligible.

## Errors

- None at construction. Negative rates are clamped to zero (silent);
  zero or negative decays default to a small positive value.
