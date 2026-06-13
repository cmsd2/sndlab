# `gain`

Linear amplitude scaling.

## Signature

```rhai
signal.gain(factor) -> Signal
// or equivalently:
gain(signal, factor) -> Signal
```

- **`factor`** — linear multiplier. `0.0` is silence; `1.0` is the
  source unchanged; `2.0` doubles. Floats and integers both work.

## Example

```rhai
sine(440.0, 1.0).gain(0.3)             // 30% of full scale
sine(440.0, 1.0).gain(0)               // silenced
mix([
    sine(220.0, 1.0).gain(0.5),
    sine(330.0, 1.0).gain(0.5),
])
```

## Clipping

There is no automatic limiter. If your final signal goes above `±1.0`
the buffer will clip when played. Keep individual layers conservative
(typically `0.2`–`0.4`) and use the master fader for level rather
than pushing individual patches loud.

## Why no dB

Decibel-aware gain (`gain_db(-6)`) is an obvious convenience and
likely to land before 1.0. For now, the linear form keeps the surface
minimal — and for the small dynamic range of game sfx, the difference
is minor.
