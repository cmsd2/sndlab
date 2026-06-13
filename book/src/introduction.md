# Introduction

**sndlab** is a TUI environment for designing procedural audio in Rust.
You write *patches* — small Rhai scripts that describe a sound — and
sndlab evaluates them, plays them through Kira, and shows you everything
in a terminal alongside a log of what's happening. It's a Strudel-style
tight iteration loop, but the patches you design here can be embedded
into shipped Rust binaries with no porting tax: the same crate that
plays them in the TUI plays them in your game.

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

## How it's built

Two crates:

- **`sndlab-core`** — the embeddable engine: a Rhai interpreter, the
  patch DSL, and Kira-based playback. Library; no UI.
- **`sndlab`** — the TUI binary: editor, log pane, project model, MCP
  server. Depends on `sndlab-core`.

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

That's all the design tooling needs to know about. The TUI handles
evaluating, playing, error reporting, and (optionally) collaborating
with an AI agent over MCP.
