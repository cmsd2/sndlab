# sndlab

[![docs](https://github.com/cmsd2/sndlab/actions/workflows/book.yml/badge.svg)](https://cmsd2.github.io/sndlab/)

A graphical sound-design environment for designing procedural audio
in Rust. Edit Rhai patches in a syntax-highlighted code editor, hear
them play through Kira, see the rendered waveform in a live scope —
and expose the buffer over MCP so an AI agent (e.g. Claude Code) can
collaborate on the same script you're looking at.

The patches you design here can be embedded directly into shipped Rust
binaries via the `sndlab-core` library crate. There is no porting tax:
the same script that designs the sound plays it in the final game.

## Status

Early development. The DSL engine compiles, the GUI renders, the
scope works, and patches play through Kira. Project model, mix model,
and MCP server are landing in order — see `book/src/SUMMARY.md` for
the roadmap of chapters and the corresponding implementation tasks.

## Quick start

```sh
# Linux only — install audio + windowing dev headers:
#   Debian/Ubuntu:  sudo apt install libasound2-dev pkg-config
#   Fedora:         sudo dnf install alsa-lib-devel pkgconf
#   Arch:           sudo pacman -S alsa-lib pkgconf

cargo run -p sndlab --release
```

That opens a window with four panels: a toolbar across the top, the
code editor in the middle-left, a live scope on the right, and a log
pane at the bottom. Hotkeys today:

| Key | Action |
|---|---|
| `F5` | Evaluate the buffer and play the first registered patch |
| toolbar buttons | Trigger any patch by name |

Quit via the window close button.

## Workspace layout

```
sndlab/
├── crates/
│   ├── sndlab-core/   # library: rhai engine + kira playback
│   └── sndlab/        # binary: eframe + egui GUI + MCP server
├── book/              # mdBook source for the user/DSL documentation
├── examples/          # example projects (coming with task 12)
├── LICENSE            # MIT
├── README.md
└── CLAUDE.md          # discipline for AI-assisted development
```

The split is deliberate: `sndlab-core` is reusable in any Rust
program that wants to load `.rhai` patches at runtime. The GUI is one
consumer; an embedded game audio engine is another (see
[Echo Field integration](./book/src/embedding/echo-field.md)).

## DSL at a glance

> Each primitive ships with its own chapter under `book/src/dsl/`. The
> table here is a quick reference; the chapters are authoritative.

| Primitive | Status | Sketch |
|---|---|---|
| `patch(name, role, signal)` | shipped | Register a named patch |
| `sine(freq_hz, dur_s)` | shipped | Sine oscillator |
| `noise(kind, dur_s)` | shipped | Noise generator (white/pink/brown) |
| `signal.env(attack_s, decay_s)` | shipped | Attack + exp decay envelope |
| `signal.gain(linear)` | shipped | Linear amplitude scaling |
| `mix([sig, ...])` | shipped | Sum signals |
| `tap(delay_s, gain)` + `signal.with_taps([...])` | shipped | Discrete delay-tap reverb |

`Status` flips to `shipped` as primitives land. Whenever a row flips,
the [DSL overview](./book/src/dsl/overview.md) status table and the
corresponding chapter both get updated in the same commit, per the
discipline in [CLAUDE.md](./CLAUDE.md).

## Documentation

Local build:

```sh
cargo install mdbook
mdbook serve book
```

Then browse <http://localhost:3000>. Online build target lives on
GitHub Pages (configuration coming with task 12).

The book is the authoritative DSL reference; this README is a tour.

## License

MIT — see [LICENSE](LICENSE).
