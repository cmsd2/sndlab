# Using sndlab-core

`sndlab-core` is the embeddable engine that sits underneath the TUI.
Any Rust binary can pull it in, load `.rhai` patches, and play them
through Kira. The TUI is *one* consumer of this surface; your game is
another.

> Pending the engine implementation (task 7). When complete, this
> chapter walks through:
>
> - Adding `sndlab-core` as a dependency.
> - Constructing an `Engine`.
> - Loading patches from a string (compile-time `include_str!`) or
>   from disk.
> - Triggering patches in response to game events.
> - Listening to the modulation surface for ambient patches.
> - Error handling and the silent-fallback story.
