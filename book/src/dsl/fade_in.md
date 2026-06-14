# `fade_in`

Smooth amplitude ramp at the *start* of a source. Complementary to
[`fade_out`](./fade_out.md).

## Signature

```rhai
signal.fade_in(seconds) -> Signal
fade_in(signal, seconds) -> Signal
```

- **`seconds`** — duration of the fade-up ramp at the start of the
  signal. After the ramp the source is passed through unchanged.

The ramp is a **sin²** curve over `[0, 1]`:

```
env(t) = sin²(π/2 · t)
```

This is the exact complement of `fade_out`'s `cos²` shape. A
`fade_in(d)` on layer A combined with a `fade_out(d)` on layer B at
the same offset reconstructs to **constant power** — useful for
crossfading between two textures without a perceptible level dip.

## Example

A torpedo emerging from underneath a bubble wash:

```rhai
patch("torpedo_emerge", "one_shot",
    mix([
        sample("samples/bubbles.wav").pitch(0.6).env(0.05, 1.5).gain(2.0),
        torpedo_layer.fade_in(0.8).fade_out(1.0),   // ramps in over 0.8 s
    ])
    .gain(0.55));
```

The torpedo rises smoothly out of silence while the bubbles play, then
fades out cleanly at the end.

## Why decouple fade-in from `env`?

`env(attack, decay)` already does a *linear* attack ramp and pairs it
with an exponential decay. That's fine when you want both shaped
together — strikes, bell-like tones — but bad when you want a long
graceful entry and either no decay or a controlled tail (`fade_out`)
shape.

| Want | Use |
|---|---|
| Hammered onset → exponential ring | `env(0.005, 2.0)` |
| Slow rise → constant body → smooth tail | `fade_in(1.0).fade_out(1.0)` |
| Crossfade between two layers | `fade_in(d)` on one, `fade_out(d)` on the other |

## Notes

- **Works on bounded and unbounded sources.** For an ambient, `fade_in`
  shapes only the opening of the stream and then passes through forever.
- **Zero or negative duration** produces no ramp (pass-through).
- **Constant-power crossfade.** Pairing `fade_in(d)` and `fade_out(d)`
  on two layers whose sum you want flat works because
  `sin²(x) + cos²(x) = 1`.

## Errors

None at construction.
