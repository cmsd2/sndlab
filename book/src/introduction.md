# Introduction

**sndlab** is a graphical environment for designing procedural audio
in Rust. You write *patches* — small Rhai scripts that describe a
sound — and sndlab evaluates them, plays them through Kira, shows
the rendered waveform in a live scope, and logs what's happening
underneath. It's a Strudel-style tight iteration loop, but the
patches you design here can be embedded into shipped Rust binaries
with no porting tax: the same crate that plays them in sndlab plays
them in your game.

## What sndlab is for

- **Designing game sound effects.** Sonar pings, hull creaks, weapon
  one-shots — anything a game engine triggers as a procedurally-
  generated buffer.
- **Designing ambient layers.** Looping, modulated background sounds
  that respond to game state (depth, speed, damage).
- **A/B-ing combinations.** Hear the sonar ping over the ocean rumble
  layer at the same time, in the same mix you'll ship.
- **AI-collaborative sound design.** sndlab exposes its editor buffer
  over MCP, so an AI assistant can edit the same script you're looking
  at and you can hear the result without leaving the loop.

## What sndlab is *not*

- **Not a DAW.** No timeline, no MIDI, no automation curves. If you
  want to write a song, this is not the tool.
- **Not a livecoding instrument.** Strudel is better at performing
  patterns; sndlab is better at iterating on a single sound until it's
  right.
- **Not a music-theory tool.** No notes, no scales, no chord progressions.

## How patches play

Every primitive in the DSL builds a node in a lazy `Signal` graph.
What happens to that graph depends on the patch's role:

- **One-shot patches** (a ping, a hit, a UI click) get *rendered* to a
  finite buffer at eval time, driven by whatever `take` / `chirp` /
  `fade_out` duration the graph specifies. The buffer plays through
  Kira's low-latency one-shot path.
- **Ambient patches** (ocean rumble, machinery hum) are *generated*:
  a fresh runner ticks the graph at audio rate for as long as the
  ambient stays enabled. There's no buffer and no loop — the graph
  just runs.

The DSL is the same either way. `sine(440)` is `sine(440)` whether
it ends up in a one-shot or an ambient — only the duration context
differs.

## How it's built

Two crates:

- **`sndlab-core`** — the embeddable engine: a Rhai interpreter, the
  patch DSL, and Kira-based playback. Library; no UI.
- **`sndlab`** — the GUI binary: an eframe window hosting a
  syntax-highlighted code editor, a waveform scope, a log pane, the
  project model, and the MCP server. Depends on `sndlab-core`.

When you ship a game, you embed `sndlab-core` and load `.rhai` patches
the same way sndlab itself does. There is no "production" version of a
patch separate from the "design" version — the same script runs in
both.

## A quick look

A complete patch looks like this:

```rhai
patch("sonar_ping", "one_shot", || {
    let body = sine(330.0, 3.5).env(0.008, 1.4).gain(0.32);
    let reverb = [
        tap(0.13, 0.55),
        tap(0.31, 0.38),
        tap(0.58, 0.26),
        tap(0.95, 0.17),
    ];
    body.with_taps(reverb)
});
```

That's all the design tooling needs to know about. The GUI handles
evaluating, playing, error reporting, and (optionally) collaborating
with an AI agent over MCP.
