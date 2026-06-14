# `loop_xfade`

Bake a self-crossfade into the buffer's last `crossfade_s` so the
loop seam doesn't click. The standard sampler technique for making
noise-based ambient buffers loopable.

## Signature

```rhai
signal.loop_xfade(crossfade_s) -> Signal
```

- **`crossfade_s`** — length of the crossfade region at the end of
  the buffer. Typical values: 0.1 to 0.5 seconds.

## When you need it

Sine waves whose frequency is an integer multiple of `1/duration`
loop seamlessly out of the box — `sin(2π · f · 0)` and
`sin(2π · f · duration)` are both exactly zero. Pure-sine ambient
patches don't need `loop_xfade`.

Noise sources don't loop. White / pink / brown noise content at the
end of the buffer is unrelated to the content at the start, so the
loop wrap is a sample-level discontinuity that reads as a quiet click
once per loop cycle. `loop_xfade` blends the buffer's tail toward its
head so the wrap is much smoother.

## Example

```rhai
patch("sub_hum", "ambient",
    mix([
        noise("brown", 4.0).lowpass(150.0, 0.707).gain(0.5),
        sine(60.0, 4.0).gain(0.15),
        sine(120.0, 4.0).gain(0.06),
    ]).gain(0.45).loop_xfade(0.2));
```

The `loop_xfade(0.2)` blends 200 ms of the tail toward the head with
a cosine-squared crossfade. The click at the loop boundary is
dramatically reduced; the remaining residual is small enough that any
filter in the chain smears it below audibility.

## Why not `fade_out`?

`fade_out` brings the buffer's end to **exact zero**. That sounds
seamless on a single play, but a looped buffer with `fade_out` has
an amplitude dip at every loop boundary — the buffer fades to zero,
then the loop restarts at full volume. The dip reads as the
ambience pulsing at the loop rate.

`loop_xfade` keeps the buffer at full level throughout, only
re-shaping the tail's *content* so the wrap is smooth. No dip, no
pulse.

## Composing with other primitives

`loop_xfade` should usually be the **last** transform applied to the
signal before `patch(...)` registers it. Anything you apply after
`loop_xfade` may re-introduce a discontinuity at the seam.

## Notes

- Capped at half the buffer length so the head and tail regions
  don't overlap.
- The crossfade is cosine-squared (smooth at both ends) so the
  blend region itself doesn't introduce its own perceptual kink.
- For highly stochastic noise (brown's random walk), the residual
  discontinuity is bounded by the noise's drift over the crossfade
  duration; longer `crossfade_s` doesn't always mean a smaller
  residual. 100–200 ms is usually the sweet spot.
