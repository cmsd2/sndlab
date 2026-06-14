# Sonar ping

The classic submarine "ping": a metallic ringing tone that decays over
a couple of seconds.

```rhai
patch("sonar_ping", "one_shot",
    mix([
        noise("white", 3.0).bandpass(1000.0, 200.0).gain(60),
        noise("white", 3.0).bandpass(2050.0, 200.0).gain(20),
    ])
    .lowpass(6000.0, 0.4)
    .tremolo(4.0, 0.2)
    .env(0.008, 1.0)
    .fade_out(0.6)
    .gain(0.25));
```

## The core trick: high-Q bandpass on white noise

The whole sound is built around one realisation: a very high-Q
bandpass on white noise reads as a *tone*, not a band of hiss. White
noise has flat spectral content; pushing it through a `bandpass(f, Q)`
with `Q ≈ 200` extracts a sliver of energy around `f` — bandwidth
roughly `f / Q`, so ~5 Hz wide at 1 kHz — which the ear hears as a
pure pitch with a tiny amount of chaotic micro-modulation. That last
detail is the point: a real sonar transducer rings with mechanical
imperfections; a pure sine sounds synthetic, a filtered-noise tone
sounds *struck*.

Two of these ringing tones at **1000 Hz** and **2050 Hz** stack into
a roughly-harmonic interval (an octave plus a quartertone), which
gives the ping a chord-like body rather than a single-tone whistle.
The lower band gets more energy (`gain(60)` vs `gain(20)`) so it
sits forward in the mix; the upper band adds shimmer.

## Why these numbers

**`bandpass(1000.0, 200.0).gain(60)`.** The huge gain compensates for
the resonant filter's narrow passband — most of the white noise is
discarded by the filter, so we crank the survivor back up to a usable
level. The Q=200 is what gives the tonal character; lower it to 20
and you hear noisy hiss instead of pitch.

**Second band at 2050 Hz.** Slightly offset from a true octave to
avoid mechanical sameness with the lower band. Gain dropped to 20 so
the upper band sits *underneath* the lower one as harmonic colour
rather than competing with it.

**`.lowpass(6000.0, 0.4)`.** Cleans any residual high-frequency hash
the bandpass filters let through. The low Q (0.4) is intentional —
it's a gentle slope, not a resonant cut.

**`.tremolo(4.0, 0.2)`.** A 4 Hz amplitude wobble at 20% depth adds a
slight pulse to the tail that reads as "this tone is alive, not a
sample loop" — sonar listeners hear similar modulation from the
transducer's mechanical hum.

**`.env(0.008, 1.0)`.** 8 ms attack avoids a hard onset click. Decay
constant 1.0 gives a 1/e time of ~1 s — the tone rings out audibly
over the following 2-3 seconds.

**`.fade_out(0.6)`.** Smooths the very end of the buffer so it doesn't
cut abruptly into silence. The env decay is asymptotic; the fade_out
brings it cleanly to zero over the last 600 ms.

**`.gain(0.25)`.** The summed bandpass-noise gains are large numbers
(60 + 20 = 80 before normalisation). The final gain dials the whole
thing back into the safe `-1.0..1.0` range with headroom for the
reverb taps you might add later.

## Why not just a chirp?

A linear-FM chirp is the textbook explanation of how real LFM sonar
pulses work, but it sounds artificial in a game. You can *hear* the
sweep — the pitch slides over the duration, and the ear immediately
labels it as "synthesised effect" rather than "metallic transducer
ringing in seawater." Bandpassed noise has the opposite quality: the
pitch is stable, but the micro-structure of the source is chaotic, so
it reads as a physical object that's been struck.

If you do want the swept character — for a wider, more "active"
sonar — `chirp(280.0, 400.0, 1.0)` is still available; see the
[chirp DSL chapter](../dsl/chirp.md). The two designs cover different
moods: filtered-noise for the iconic submarine ping; chirp for a
modern active-sonar pulse.

## Variants

- **Brighter / more urgent.** Move both bandpass centres up:
  `bandpass(1400, 200)` + `bandpass(2800, 200)`. Reads as a smaller
  boat, lighter weapon.
- **Heavier / older sub.** Drop both centres: `bandpass(600, 200)` +
  `bandpass(1250, 200)`. Sounds like a WWII-era ASDIC set.
- **Pure single tone.** Drop the upper band entirely. The ping sounds
  more like a research pinger or a beacon.
- **Tighter interval (octave).** Set the upper band to exactly twice
  the lower (1000 → 2000). The harmonic alignment makes the two
  bands fuse into one tone with extra brightness rather than reading
  as a chord.
- **Longer ring.** Halve the env decay constant: `.env(0.008, 0.5)`
  gives a ~2 s 1/e time. Pair with a longer `fade_out(1.2)`.

## Test in sndlab

Drop the patch into the editor pane, press F5. The scope's upper pane
should show a slowly-decaying ringing waveform; the lower spectrum
pane should show two narrow peaks near 1 kHz and 2 kHz — the two
bandpass centres — sitting above a low noise floor. If you see
broadband hiss instead of distinct peaks, the bandpass Q is too low.
