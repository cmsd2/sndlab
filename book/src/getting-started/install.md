# Install and run

## Prerequisites

- A recent stable Rust toolchain (`rustc --version` ≥ 1.75).
- An audio device the operating system can open.
- A graphical display (X11 or Wayland on Linux, native windowing on
  macOS and Windows).
- **Linux only:** ALSA dev headers (`libasound2-dev` on Debian/Ubuntu,
  `alsa-lib-devel` on Fedora, `alsa-lib` on Arch). Kira builds against
  them via cpal. eframe's default desktop-window features cover X11
  and Wayland; if you're cross-compiling for an embedded target you
  may need to enable them explicitly.

## Build and launch

```sh
git clone https://github.com/cmsd2/sndlab
cd sndlab
cargo run -p sndlab --release
```

A window opens with four panels:

- **Toolbar** across the top — Eval+Play button, plus a per-patch
  trigger button for every patch registered by the current script.
- **Editor** (centre-left) — syntax-highlighted code editor for the
  current `.rhai` file.
- **Scope** (right) — live oscilloscope showing the last rendered
  patch buffer. Updates whenever you eval.
- **Log** (bottom) — evaluation results, audio playback notifications,
  warnings, errors.

## Hotkeys

| Key | Action |
|---|---|
| `F5` | Evaluate the buffer and play the first registered patch |
| toolbar buttons | Trigger any patch by name |

Quit via the window close button.

## Building the docs

```sh
cargo install mdbook
mdbook serve book
```

Then point your browser at <http://localhost:3000>.
