# Sonar ping

The classic submarine "ping": a swept tone that rings out and bounces
off distant geometry as discrete reflections.

```rhai
patch("sonar_ping", "one_shot",
    chirp(280.0, 400.0, 1.0).env(0.008, 1.4).gain(0.32)
        .with_taps([
            tap(0.13, 0.7),
            tap(0.31, 0.5),
            tap(0.58, 0.35),
            tap(0.95, 0.22),
        ]));
```

## Why these numbers

**Chirp 280 → 400 Hz over 1 s.** The sweep is a small fraction of an
octave — enough to give the source broadband content (so the taps don't
collapse into a comb filter) without making the pitch shift obvious.
Real navy LFM pulses sweep similarly: a few hundred Hz of bandwidth
over half a second to a few seconds. See [`chirp`](../dsl/chirp.md)
for the rationale.

**`env(0.008, 1.4)`.** 8 ms attack avoids a hard onset click. Decay
constant 1.4 gives a 1/e time of ~700 ms — the tone rings out audibly
over the following second.

**`gain(0.32)`.** Keeps the dry signal well below `1.0` so the summed
taps won't clip.

**Taps at 0.13 / 0.31 / 0.58 / 0.95 s.** Roughly geometric spacing so
the reflections feel like progressively further bounces rather than a
regular echo.

**Tap gains decreasing 0.7 → 0.22.** Each reflection a bit quieter
than the last — energy loss with distance.

**Default per-tap decay (~80 ms).** Each tap is a brief reflection,
not a sustained replay. See [`tap`](../dsl/tap.md) for the decay
semantics.

## Why not just a sine?

A pure sine plus delayed copies produces a comb filter: at frequencies
where dry and tap are in phase you get a boost; out of phase you get
a sustained null. With one source frequency that null is *at* that
frequency, audible as a dropout when a tap kicks in (the dry signal
narrows then opens out again). The chirp distributes the source's
energy across a band, so the comb's nulls and peaks sweep through
frequencies in step with the chirp and average out perceptually.

## Variants

- **Higher pitch / wider sweep (`chirp(440, 880, 0.4)`)** reads as a
  smaller boat, lighter weapon, more "active" character.
- **Downward sweep (`chirp(450, 280, 1.2)`)** reads as a passive
  pulse or distant bottom return — the falling pitch feels heavier.
- **Tighter reflections (`tap(0.07, 0.7)`, `tap(0.12, 0.5)`)** read
  as a smaller enclosed space — close walls, narrow canyon.
- **Stretched reflections (`tap(0.4, 0.7)`, `tap(0.9, 0.5)`)** read
  as a deep open ocean — distant bottom or thermocline reflections.

## Test in sndlab

Drop the patch into the editor pane, press F5. The scope's upper pane
should show a smooth swept ringing tone; the lower spectrum pane
should show the chirp's bandwidth (~120 Hz wide, centred just above
300 Hz) plus a quieter tail of harmonics from the envelope's edges.
