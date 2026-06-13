# `mix`

Sum an array of signals into a single buffer.

## Signature

```rhai
mix([sig_a, sig_b, ...]) -> Signal
```

The argument is a Rhai array; every element must be a `Signal`.

The output length is `max(len(sig) for sig in inputs)`. Shorter
inputs are zero-padded at the end. Samples are summed without any
normalisation — the caller is responsible for keeping the result
below `±1.0` (typically by scaling each layer with `.gain(...)`).

## Example

```rhai
// Two harmonics summed:
patch("dyad", "one_shot",
    mix([
        sine(220.0, 2.0).gain(0.3),
        sine(330.0, 2.0).gain(0.3),
    ]));

// A pinged tone with a noise burst layered on top:
mix([
    sine(440.0, 0.4).env(0.005, 5.0).gain(0.5),
    noise("white", 0.05).env(0.0, 50.0).gain(0.3),
])
```

## Errors

- `mix: element N is not a Signal` — one of the array elements
  wasn't a `Signal`. Common cause: forgetting to wrap a number in
  `sine(...)` or similar.

## Notes

- An empty array produces an empty buffer (`mix([])` returns a
  zero-length `Signal`).
- Stereo panning is not exposed by `mix`. Per-source panning lives in
  the mix model and is applied at play time, not at synthesis time.
