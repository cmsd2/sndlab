# `highpass`

A biquad high-pass filter. The mirror of [`lowpass`](./lowpass.md):
passes content above `cutoff_hz` and attenuates content below.

## Signature

```rhai
signal.highpass(cutoff_hz, q) -> Signal
```

- **`cutoff_hz`** — the −3 dB knee.
- **`q`** — resonance at the knee. **`0.707`** for Butterworth (no
  peak). Higher Q emphasises the cutoff frequency.

## Examples

```rhai
// Cut everything below 100 Hz — kills room rumble or DC offset.
signal.highpass(100.0, 0.707)

// Aggressive "thinned out" tone — keeps just the high content.
signal.highpass(2000.0, 0.707)

// Resonant highpass at 4 kHz — the inverse of a resonant lowpass,
// rare in design but useful for whistle-like character.
signal.highpass(4000.0, 6.0)
```

## When to use it

- **Remove low-frequency rumble.** Sources from real recordings often
  carry sub-bass content (room hum, mic handling, DC offset). A high-
  pass at 60–120 Hz removes it without affecting the audible part.
- **Brighten a bass-heavy source.** When `brown` noise or a deep sine
  is the source, a highpass leaves only the upper edge — useful as
  a "sizzle" layer in additive synthesis.
- **Complement a lowpass.** A `signal.highpass(800).lowpass(4000)`
  pair makes a wide bandpass with independent Q at each side.

## Notes

The same caveats as [`lowpass`](./lowpass.md): 12 dB/octave roll-off,
brief transient response, and use multiple in series for sharper
cuts.
