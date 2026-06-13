# `tremolo`

Periodic amplitude modulation. A low-frequency sine wave is multiplied
into the signal, making the loudness pulse over time. Often used to
add "life" to a sustained tone — a steady ring becomes a slowly
breathing ring.

Compare with vibrato (which would modulate **pitch**, not amplitude
— a different operation that isn't currently in the DSL).

## Signature

```rhai
signal.tremolo(rate_hz, depth) -> Signal
```

- **`rate_hz`** — modulation frequency. Musically useful range is
  **3–8 Hz**. Below 1 Hz feels like volume swelling; above ~15 Hz
  starts to sound like AM synthesis rather than tremolo.
- **`depth`** — 0 to 1. `0` is no modulation; `1` swings the
  amplitude between 0 and the input's full level (100 % tremolo).
  Subtle musical settings sit around 0.3–0.5.

The LFO starts at full amplitude (`cos(0) = 1`) so the signal's onset
is not attenuated. The phase reaches the minimum after half a period.

## Example

```rhai
// A 1700 Hz ring with a gentle 4 Hz tremolo wobble.
patch("ping", "one_shot",
    sine(1700.0, 2.5)
        .env(0.008, 1.2)
        .tremolo(4.0, 0.3)
        .fade_out(0.4));

// Heavy tremolo as a "stuttering" effect.
patch("stutter", "one_shot",
    sine(220.0, 1.0).tremolo(10.0, 0.95));
```

## Composing with `env`

`env` shapes the body decay; `tremolo` lays a wobble on top. They
multiply, so the order doesn't matter mathematically — `.env(...).tremolo(...)`
and `.tremolo(...).env(...)` produce identical output. Read it
whichever way scans better.

## Notes

- **Tremolo on filtered noise** modulates the **whole noise band**,
  not the bandpass centre frequency. The result is the entire shaped
  spectrum pulsing in volume. For a "pitch-wobble" effect on
  filtered noise you'd need a time-varying filter, which isn't in
  the DSL yet.
- **Stacking tremolo and fade_out is fine.** Tremolo modulates
  ongoing amplitude; fade_out catches whatever's left at the end.
  The combination produces a wobbling tone that decays cleanly to
  silence.
- **`depth` above 1 clamps to 1.** No hard limit on `rate_hz` but
  rates above the Nyquist limit will alias.
