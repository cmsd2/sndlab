# `sample`

Load a decoded audio file (MP3 / WAV / Ogg / FLAC) as a DSL `Signal`,
then process it with the rest of the chain — filters, envelope,
gain, taps, mix layering.

## Signature

```rhai
sample(path)        -> Signal   // plays once, ends
sample_loop(path)   -> Signal   // wraps at end-of-buffer, forever
```

- **`path`** — string. Absolute paths are used as-is. Relative paths
  resolve **against the project root** (the directory holding
  `project.ron`), independently of where sndlab was launched from. If
  the project hasn't been saved yet there is no root, and relative
  paths produce a friendly error pointing you to save the project or
  use an absolute path.

The file is decoded once at patch-registration time (during `eval`).
Decoded samples are mono (multichannel sources are averaged) and
linearly resampled to the engine's 48 kHz at playback. Cheap; loading
is amortised across every play.

## Examples

A torpedo launch with a sampled mechanical click and an algorithmic
whoosh underneath:

```rhai
patch("torpedo_launch", "one_shot",
    mix([
        sample("samples/breech_click.wav").gain(0.6),
        noise("pink", 0.6).bandpass(1500.0, 1.0)
            .env(0.005, 4.0).gain(0.3),
    ]).fade_out(0.15));
```

A bubbling wake using a Freesound loop with our filters laid on top:

```rhai
patch("torpedo_wake", "ambient",
    sample_loop("samples/freesound_bubbles_loop.wav")
        .lowpass(3500.0, 0.707)     // tame the top-end hiss
        .gain(0.4));
```

## Composing with other primitives

`sample(...)` returns a `Signal` like anything else, so the entire
chain is available:

- `.pitch(factor)` — sample-only tape-speed control. `0.5` = octave
  down + double duration; `2.0` = octave up + half duration; `1.0`
  is the original. Composes by multiplication, so
  `.pitch(0.5).pitch(0.5)` = `0.25` (two octaves down). Repitched
  samples report their adjusted duration to `fade_out`, so a
  pitched-down rush still fades over its full new length.
- `.lowpass(...)` / `.highpass(...)` / `.bandpass(...)` — colour the
  sample
- `.gain(...)` — set level
- `.env(attack, decay)` — re-shape the amplitude envelope (useful for
  punching transients onto looped textures)
- `.fade_out(duration)` — apply a tail fade (one-shot only — needs a
  finite source)
- `.tremolo(rate, depth)` — amplitude modulate
- `.with_taps([...])` — feed into reverb taps
- `mix([...])` — layer with algorithmic sources

`pitch` only makes sense on a Sample — applying it to a sine, noise,
chirp, or grains source errors at eval time. (Use `sine`'s `freq_hz`
argument if you want to "pitch" a sine.)

## When to use which

| | `sample(...)` | `sample_loop(...)` |
|---|---|---|
| Patch role | one-shot | ambient |
| End of buffer | terminates | wraps to start |
| Use for | impacts, launches, single events | bubble loops, machinery beds, textures |

A `sample(...)` in an ambient patch will play through once and then
go silent — useful but probably not what you want. A
`sample_loop(...)` in a one-shot patch hits the 10 s safety cap and
gets cropped. Match the function to the role.

## Path resolution

The path resolver works like this:

```
sample("samples/bubbles.wav")
└─ relative? ────► yes  ─► join with project.root
                  no   ─► use the absolute path
```

If the project has never been saved (no `project.ron`), there is no
root, and relative paths error. Save the project first
(`Project → Save As…`) or pass an absolute path.

The reference loader (`Load reference…` in the toolbar) uses the same
decoder under the hood, so anything sndlab can show on the scope is
something `sample(...)` can play.

## Freesound workflow

1. Download a clip from Freesound. Look for a CC0 or CC-BY licence so
   you can ship it with your game.
2. Drop it in your project directory, e.g.
   `my-project/samples/bubbles.wav`.
3. Reference it from a patch as `sample("samples/bubbles.wav")`.
4. Eval. The first eval after the file lands does the decode; further
   evals reuse the cached decode (until you re-eval, at which point
   we decode again — `sample()` is decode-per-eval, not decode-once-
   ever).

## Errors

- `sample('foo.wav'): relative path but no project root is set — save the project first, or use an absolute path` — see "Path resolution".
- `sample('/abs/path.wav'): file open: No such file or directory (os error 2)` — typo or wrong relative root.
- `sample('foo.weird'): symphonia: unsupported format` — only MP3 / WAV / Ogg / FLAC are wired up. Convert with `ffmpeg`.

## Notes

- **Mono fold.** Stereo sources are folded down to mono. If you need
  the panning, you can split-load by writing two `.wav` channel files,
  but the engine is mono-only for now.
- **Caching.** The decoder runs once per `eval` per `sample(...)`
  call site. Re-eval re-decodes; for big files this can stutter the
  edit loop. Move heavy samples out of the patch into a separate
  manifest later if this hurts.
- **Determinism.** Sample playback is deterministic — same file, same
  output. No PRNG, no warmup.
