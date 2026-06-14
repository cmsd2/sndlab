# `delay`

Prepend silence before the source. Useful for staggering elements in
a mix without baking silence into the source.

## Signature

```rhai
signal.delay(seconds) -> Signal
delay(signal, seconds) -> Signal
```

- **`seconds`** — how much silence to prepend, in seconds. Zero or
  negative is allowed and produces no delay.

The returned signal:
- Outputs `seconds × 48 000` zero samples first.
- Then delegates to the source until it ends (one-shots) or forever
  (ambients).
- Reports `finite_duration_s() = source_duration + seconds` for the
  one-shot renderer, so wrapping `fade_out(...)` around a delayed
  source still fades the right region.

## Example

Stagger the entry of a layer in a mix:

```rhai
patch("entry_after_pad", "one_shot",
    mix([
        sine(220.0, 4.0).env(0.05, 0.4).gain(0.2),         // starts at t=0
        sine(440.0, 4.0).env(0.05, 0.4).gain(0.2)          // starts at t=1.5
            .delay(1.5),
    ])
    .fade_out(0.8)
    .gain(0.6));
```

The two sines mix together but the upper one only enters 1.5 s in.
Without `delay(...)` you would have to bake silence into the second
sine's source, which loses composability — `delay` keeps every layer
expressible as a fluent chain.

## Notes

- **Per-layer staggering.** `delay` is most useful inside `mix([…])`.
  At the top level it's just a leading silent gap, which the host
  could also achieve by holding off the trigger.
- **Doesn't affect pitch or speed.** This is timeline offset, not
  audio rate scaling. For pitch-/speed-changing wrappers see
  [`pitch`](./sample.md) and [`speed`](./sample.md).
- **Works on ambients.** A delayed ambient emits silence for the
  initial period then runs forever. The host's stop/fade behaviour
  is unchanged.

## Errors

None at construction. Negative durations are clamped to zero.
