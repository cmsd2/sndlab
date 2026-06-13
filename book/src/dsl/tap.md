# `tap`

A single delay tap, used with `with_taps` to build discrete reverb tails.

## Signature

```rhai
tap(delay_s, gain) -> Tap

signal.with_taps([tap_a, tap_b, ...]) -> Signal
```

A `Tap` is just a `(delay_s, gain)` pair until you hand it to
`with_taps`, which copies the source buffer at each tap's offset with
the tap's gain, summed back into the source.

The output buffer is extended by the longest tap delay so the tail
has somewhere to live.

## Example

```rhai
patch("sonar_ping", "one_shot",
    sine(330.0, 3.5).env(0.008, 1.4).gain(0.32)
        .with_taps([
            tap(0.13, 0.55),
            tap(0.31, 0.38),
            tap(0.58, 0.26),
            tap(0.95, 0.17),
            tap(1.45, 0.10),
            tap(2.05, 0.06),
        ]));
```

That's the classic submarine ping: a sine body with six discrete
reflections, each later and quieter, simulating sound bouncing off
distant geometry.

## Errors

- `with_taps: element N is not a Tap` — one of the array elements
  wasn't built with `tap(...)`.

## Why discrete taps, not convolution reverb

A real reverb convolves the source with an impulse response. That's
expensive (FFT-domain or 5000+ taps) and overkill for a game-shaped
reverb tail. Six discrete taps render in microseconds and give the
"near reflections then late tail" character listeners expect.

A proper convolution reverb will land later when the recipes need it
(probably for cathedral / open-ocean sounds). For sonar-room
geometry, discrete taps are the right tool.
