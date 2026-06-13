# Echo Field integration

[Echo Field](https://github.com/cmsd2/echo-field) is the submarine
simulation game sndlab was originally built to support. Its audio
architecture (see `docs/audio-design.md` in that repo) calls for
procedurally synthesised sonar pings, ambient layers, and per-contact
voices — exactly the shape sndlab's DSL is designed for.

> Pending the engine implementation and Echo Field's audio module
> migration. The plan:
>
> 1. Echo Field declares `sndlab-core` as a dependency.
> 2. Audio patches live in `crates/app/src/audio/patches/*.rhai` (or
>    embedded via `include_str!` for release builds).
> 3. At game startup, the audio engine evaluates the bundled scripts
>    and registers patches by name.
> 4. Game events (`SimEvent::TorpedoFired`, …) trigger one-shots; the
>    listener-state modulation surface drives ambient parameters.
> 5. The TUI sndlab session and the game share the same patch source —
>    designing in sndlab and shipping in the game are the same act.
