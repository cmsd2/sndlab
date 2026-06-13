# Buffers and sample rate

A **buffer** is the rendered output of a patch: a contiguous block of
audio samples, plus the sample rate they were rendered at. All buffers
in sndlab are mono at synthesis time; stereo is applied by the mixer
when the buffer plays.

## Sample rate

By default sndlab renders at the **audio device's preferred sample
rate** (typically 48000 Hz on modern hardware, 44100 Hz on older
hardware). Patches don't need to know this — the DSL works in seconds
and Hertz, not samples. The engine picks the right number of samples
when the buffer is rendered.

If you need to override the rate (e.g. for offline rendering at a
fixed rate for tests), the project manifest exposes a
`render_sample_rate` field. See [The project manifest](../projects/manifest.md).

## Buffer lengths

Patch length is determined by the patch body. A one-shot is finite —
it has a natural end (envelope decays, taps run out). An ambient
patch can describe a fixed-length region that the mixer loops, or it
can render a long buffer directly. The DSL has no concept of "infinite
streams" yet; everything is pre-baked into a buffer at evaluation time.

## Mono in, stereo out

The DSL produces a mono signal. The mixer applies panning per-source
when the buffer is played (see [The mix graph](./mix-graph.md)). This
keeps the synthesis side simple and lets the same buffer be played at
different positions in the field without re-rendering.
