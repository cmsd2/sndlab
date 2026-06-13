# Install and run

## Prerequisites

- A recent stable Rust toolchain (`rustc --version` ≥ 1.75).
- An audio device the operating system can open.
- **Linux only:** the ALSA development headers (`libasound2-dev` on
  Debian/Ubuntu, `alsa-lib-devel` on Fedora, `alsa-lib` on Arch). Kira
  builds against them via cpal; without them the build fails with
  *"Package alsa was not found in the pkg-config search path."*

## Build and launch

```sh
git clone https://github.com/cmsd2/sndlab
cd sndlab
cargo run -p sndlab
```

You should land in a TUI with an editor pane, a status bar, and a log
pane. The status bar shows buffer size and (eventually) the MCP
endpoint.

## Quitting

`Ctrl+Q` quits cleanly. The TUI restores the terminal on the way out.

## Building the docs

```sh
cargo install mdbook
mdbook serve book
```

Then point your browser at <http://localhost:3000>.
