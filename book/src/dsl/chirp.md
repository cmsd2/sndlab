# `chirp`

A linear-frequency-modulated sine wave (LFM). Frequency interpolates
linearly from `start_hz` to `end_hz` over `duration_s` seconds.

## Signature

```rhai
chirp(start_hz, end_hz, duration_s) -> Signal
```

All three arguments accept integers or floats. Internally everything
is `f32` at 48 kHz.

- **`start_hz`** — instantaneous frequency at the start of the buffer.
- **`end_hz`** — instantaneous frequency at the end of the buffer.
- **`duration_s`** — buffer length in seconds.

Amplitude is unit (`±1.0`). Use `.gain(...)` to scale.

## Example

```rhai
// Active sonar ping: ~quarter-octave sweep up, 1 second long
chirp(280.0, 400.0, 1.0).env(0.008, 1.4).gain(0.32)

// Whoop alarm: downward sweep an octave wide, fast
chirp(880.0, 440.0, 0.15).env(0.005, 5.0).gain(0.5)

// Constant tone — equivalent to sine(440, 1.0)
chirp(440.0, 440.0, 1.0)
```

## Why this exists

A pure sine is monochromatic: all of its energy is at one frequency.
When you sum delayed copies of a sine (via [`with_taps`](./tap.md)),
the result is a stable [comb filter](https://en.wikipedia.org/wiki/Comb_filter):
at certain frequencies the dry and the taps add constructively
(louder); at others they cancel destructively (quieter, audible
nulls). For a pure sine all the energy sits *at* one of those
frequencies and you hear it as either a sustained boost or — worse —
a sustained dropout.

A chirp covers a band of frequencies over time. Delayed copies of a
chirp also cover that band but with a time offset, so the constructive
and destructive points sweep through frequency in step with the
chirp. The interference is no longer locked to a single audible null;
it averages into a reverb-like texture.

For game audio, chirps are the right primitive for anything modelled
on real sonar (active pings, whoop alarms, weapon-launch signatures)
because real underwater pulses are also chirped — broadband on
purpose, for the same reason.

## Notes

- The phase at `t = 0` is zero, so the buffer starts at sample value
  `0`. No click on onset before envelope.
- Going **downward** (`start_hz > end_hz`) is fine. The math just
  works in reverse.
- Going through zero (`start_hz` and `end_hz` straddle 0) is a
  pathological case: aliasing-style artefacts as the instantaneous
  frequency dips below 0 Hz. Don't do that — use [`sine`](./sine.md)
  with a slow LFO if you actually want zero-crossings of a tone's
  frequency.
- Logarithmic / exponential chirps (perceptually-linear sweeps used in
  audio measurement) aren't supplied yet. They land when a recipe
  calls for them.
