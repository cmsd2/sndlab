# `fade_out`

Apply a smooth cosine-squared fade to the last `duration_s` of the
buffer. Composes with [`env`](./env.md) for the common "long ring
that needs a graceful tail" shape.

## Signature

```rhai
signal.fade_out(duration_s) -> Signal
```

- **`duration_s`** — length of the fade region, measured from the
  end of the buffer.

The first `len(signal) − duration_s` of the buffer is unchanged. Over
the last `duration_s` the amplitude follows `cos²(π·t/2)` where
`t` runs 0 → 1 — smooth at both ends (no audible kink where the fade
begins, exact zero at the end).

## When to use it

The single exponential `env` decays forever; if the buffer is shorter
than the envelope's natural die-off, the buffer terminates with the
envelope still at audible amplitude and you hear a click on playback.
You have two choices:

1. **Extend the buffer until the envelope is inaudible** — wasteful
   for long-ring sounds; you're storing seconds of near-silent
   samples.
2. **Use `fade_out`** — keep the buffer as long as you actually want
   to hear the sound, and let `fade_out` mop up whatever amplitude is
   left at the end.

## Example: long ring with graceful tail

```rhai
patch("ping", "one_shot",
    sine(440.0, 2.5)
        .env(0.008, 1.2)            // body: slow exponential decay
        .fade_out(0.4));            // tail: smooth ramp to silence
                                    //       over the last 400 ms
```

The body envelope decays at its natural exponential rate; the fade
catches whatever amplitude remains in the last 400 ms and brings it
cleanly to zero.

## Notes

- `fade_out(duration_s)` truncated to buffer length. Pass a `duration_s`
  bigger than the buffer and the entire buffer gets faded.
- `fade_out(0)` or negative is a no-op.
- The cosine-squared shape is the audio-engineering standard for clean
  fades. Linear fades have a perceptual "kink" where the slope changes;
  this one's smooth all the way through.
