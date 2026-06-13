# Sonar ping

The classic submarine "ping": a low-pitched sine that rings out and bounces
off distant geometry as discrete reflections.

```rhai
patch("sonar_ping", "one_shot",
    sine(330.0, 1.5).env(0.008, 1.4).gain(0.32)
        .with_taps([
            tap(0.13, 0.7),
            tap(0.31, 0.5),
            tap(0.58, 0.35),
            tap(0.95, 0.22),
        ]));
```

## Why these numbers

**Frequency: 330 Hz.** Low enough to read as "naval sonar" rather than as
"UI beep." Adjustable — see "variants" below.

**Source duration: 1.5 s.** Long enough that the sustained ringing tone
characterises the sound, short enough that the buffer doesn't grow huge.

**`env(0.008, 1.4)`.** 8 ms attack avoids a hard onset click. Decay constant
1.4 gives a 1/e time of ~700 ms — the tone rings out audibly over the
following second.

**`gain(0.32)`.** Keeps the dry signal well below `1.0` so the summed taps
won't clip.

**Taps at 0.13 / 0.31 / 0.58 / 0.95 s.** Roughly geometric spacing so the
reflections feel like progressively further bounces rather than a regular
echo.

**Tap gains decreasing 0.7 → 0.22.** Each reflection a bit quieter than the
last — energy loss with distance.

**Default per-tap decay (~80 ms).** Each tap is a brief reflection of the
source's onset, not a sustained replay. This is what makes the result sound
like a ping bouncing back rather than four overlapping organ tones. See
[`tap`](../dsl/tap.md) for the full decay semantics.

## Variants

- **Higher pitch (`sine(440, ...)`)** reads as a smaller boat, lighter
  weapon, faster vessel.
- **Longer body (`sine(330, 3.0)`)** sustains the dry signal further; the
  reflections still die in 80 ms each, but the overall ring is longer.
- **Tighter reflections (`tap(0.07, 0.7)`, `tap(0.12, 0.5)`)** read as a
  smaller enclosed space — close walls, narrow canyon.
- **Stretched reflections (`tap(0.4, 0.7)`, `tap(0.9, 0.5)`)** read as a
  deep open ocean — distant bottom or thermocline reflections.

## Test in sndlab

Drop the patch into the editor pane, press F5. The scope should show a sharp
attack and a long decay, with three or four secondary attacks visible on the
right where the reflections kick in. Audibly: a single low ping followed by
several echoes that die quickly.
