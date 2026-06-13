# `env`

Apply an attack ramp and exponential decay envelope to a signal.

## Signature

```rhai
signal.env(attack_s, decay_s) -> Signal
// or equivalently:
env(signal, attack_s, decay_s) -> Signal
```

- **`attack_s`** — linear ramp from silence to full at the start of
  the buffer. `0.0` means "no ramp" (the buffer starts at full
  amplitude on sample 0).
- **`decay_s`** — exponential decay time constant. Larger values =
  longer ring. The envelope falls to `1/e` (≈ 37%) after `decay_s`
  seconds and to ~5% after `3 × decay_s`.

The envelope is applied multiplicatively per-sample; the source
buffer's length is unchanged.

## Example

```rhai
// A quick "click" that decays to inaudible in ~150 ms:
sine(2000.0, 0.2).env(0.001, 30.0)

// A long ringing tone — 4-second time constant means it's still
// at ~6% after 12 seconds. Combine with a finite buffer length.
sine(220.0, 8.0).env(0.01, 4.0).gain(0.4)

// No attack (hard onset, used for percussive transients):
noise("white", 0.05).env(0.0, 100.0).gain(0.5)
```

## Why exponential

Exponential decay matches the way natural resonators (strings, bells,
hulls) lose energy and is what listeners associate with "ringing".
Linear decay sounds artificial — it stays loud and then suddenly
stops.

## Future variants

A separate ADSR envelope (attack/decay/sustain/release) and a
data-driven multi-segment envelope will land when patches that need
them justify the API surface.
