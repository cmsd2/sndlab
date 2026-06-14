# `sine`

A pure sine wave at a given frequency.

## Signature

```rhai
sine(freq_hz)              -> Signal   // unbounded; runs forever in an ambient
sine(freq_hz, duration_s)  -> Signal   // bounded; equivalent to sine(freq_hz).take(duration_s)
```

Both arguments accept integers or floats. Internally everything is
`f32` at 48 kHz; the script is free to use either numeric type.

- **`freq_hz`** — frequency in Hertz.
- **`duration_s`** — optional. When supplied, the source terminates
  after this many seconds (one-shots use this to bound their
  rendered buffer; ambients ignore the bound and run forever).

Amplitude is unit (`±1.0`). Use `.gain(...)` to scale.

## Example

```rhai
let pure_a4 = sine(440.0, 1.0);                  // A above middle C, 1 second
let scaled  = sine(440.0, 1.0).gain(0.5);        // half amplitude
let chime   = sine(880, 0.4).env(0.005, 4.0);    // short, fast attack, ringing decay
```

## Notes

- Phase starts at zero. Successive `sine` calls don't share phase —
  each is an independent buffer.
- Negative or zero `duration_s` produce an empty buffer (length 0)
  with no error.
- Very low frequencies (< ~10 Hz) are valid but you won't hear them
  on most playback hardware; they're useful as control signals for
  modulating other layers (a use case the current DSL doesn't expose
  directly — coming with the modulation work).
