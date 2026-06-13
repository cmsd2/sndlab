# `bandpass`

A biquad bandpass filter. Pass it a broadband signal (typically noise)
and it carves out a resonant peak at `center_hz`, attenuating
everything outside the band.

## Signature

```rhai
signal.bandpass(center_hz, q) -> Signal
```

- **`center_hz`** — the centre of the passband.
- **`q`** — the resonance / Q factor. Higher means a narrower peak.
  Roughly: bandwidth ≈ `center_hz / q`. Useful range 0.5 (very broad,
  about an octave wide) to 50 (very narrow, almost a sine).

The filter uses the constant-skirt-gain bandpass coefficients from the
RBJ Audio EQ Cookbook — the standard biquad for this job in audio
software.

## Why this exists

The DSL's source primitives — `sine`, `chirp`, `noise` — are either
spectrally pure (sines, chirps) or perfectly flat (noise). Realistic
sounds usually have **shaped spectra** that are neither: a hum tone at
some specific frequency *with side-lobes around it*, or a broadband
crackle *that rolls off above a certain frequency*. You get those
shapes by filtering broadband content rather than by summing pure
tones.

The classic example is a **resonant body** — a bell, a sonar
transducer housing, a tube. Strike it with broadband energy (a hammer
hit, an electrical impulse) and the body's resonance carves the
broadband spectrum into one or more peaks. We model that as
`noise(...).bandpass(...)`.

## Example: a single-peak ping

```rhai
patch("hum", "one_shot",
    noise("white", 1.0)
        .bandpass(1000.0, 12.0)        // 1 kHz peak, ~80 Hz wide
        .env(0.005, 1.2)
        .gain(0.5));
```

That's a 1 kHz tonal hum with a noisy texture — the noise inside the
passband shows up as "jagged" amplitude variation across the peak in
an FFT, distinct from the pinpoint spike a pure sine would produce.

## Example: subtractive ping with two peaks

```rhai
patch("ping", "one_shot",
    mix([
        noise("white", 1.0).bandpass(1000.0, 12.0).gain(0.5),  // 1 kHz fundamental
        noise("white", 1.0).bandpass(2050.0, 12.0).gain(0.4),  // 2 kHz, octave up
        noise("white", 1.0).bandpass(3200.0, 1.5).gain(0.15),  // broad shoulder
    ]).env(0.008, 1.5).gain(0.45));
```

The first two filters create narrow resonances; the third uses a low Q
to scoop out a broader high-frequency shoulder. This is the shape of
many real metallic/transducer pings — broadband excitation through a
multi-mode resonant body.

## Notes

- **Transient response.** The biquad needs a few sample periods to
  settle from cold; the first ~5 ms of the output is a brief
  attack-like ramp. Usually masked by `env(...)` so it doesn't matter.
- **Very high Q can ring.** At Q above ~50 the filter is essentially a
  resonator: it'll continue ringing after the input ends. Sometimes
  this is what you want (struck-bell-like sustain); sometimes it's an
  artefact. Drop Q if you don't want it.
- **Bandpass is a single tool.** Combine multiple `bandpass` calls in
  parallel via `mix(...)` to build complex spectral shapes — that's
  the workhorse pattern. Sequential bandpasses (one feeding another)
  don't usually do what you want.
- **Out-of-band content goes to roughly silence.** The filter rolls
  off at 6 dB/octave per pole; the biquad gives you 12 dB/octave on
  each side of the peak. Below the centre by an octave, the level
  drops about 20 dB.
