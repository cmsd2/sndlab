# `lowpass`

A biquad low-pass filter. Passes content below `cutoff_hz` and
attenuates content above.

## Signature

```rhai
signal.lowpass(cutoff_hz, q) -> Signal
```

- **`cutoff_hz`** — the −3 dB knee of the filter. Content well below
  this passes essentially unchanged; well above it attenuates at
  12 dB/octave.
- **`q`** — resonance at the cutoff knee. **`0.707`** is the standard
  Butterworth value (no audible peak; the cleanest response). Higher
  values (2–10) create an emphasis right at the cutoff, often used
  for the "synth lead" tone. Beyond ~10 the filter self-resonates and
  may ring after the input ends.

The biquad uses the RBJ Audio EQ Cookbook coefficients, the same
formulas in nearly every audio plugin.

## Examples

```rhai
// Cut everything above 5 kHz — kills breathy high-frequency noise
// from a noisy source. Butterworth Q for a clean roll-off.
noise("white", 1.0).lowpass(5000.0, 0.707)

// Subtle "muffle" of an existing signal — drops the top end without
// killing presence.
signal.lowpass(8000.0, 0.707)

// Resonant low-pass at 800 Hz — emphasises that frequency while
// rolling off everything above.
signal.lowpass(800.0, 6.0)
```

## When to use it

- **Tame a noise source.** White noise is bright by default; a lowpass
  at a few kHz turns it from hiss into rumble.
- **Simulate distance / absorption.** High frequencies attenuate
  faster over distance in water and air. A lowpass models that.
- **After bandpass synthesis.** When you've carved a tonal shape from
  noise with `bandpass`, a lowpass on the sum can tidy up any
  residual high-frequency content that leaked through.

## Notes

- **12 dB/octave roll-off.** A biquad is 2nd-order, so the attenuation
  rate is 12 dB per octave above the cutoff. For sharper cuts, apply
  multiple lowpasses in series (each adds another 12 dB).
- **Transient response.** The biquad needs a few sample periods (~3 ms)
  to settle from cold; the first samples of the output are a brief
  ramp. Usually masked by `env(...)`.
- **Don't use to "remove highs from the spectrum view."** If the FFT
  is showing energy above the cutoff, you're seeing residual content
  after a single biquad. Either run it through two lowpasses or pick
  a tighter cutoff.
