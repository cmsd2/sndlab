# sndlab

A TUI sound-design environment for designing procedural audio in Rust.
Edit Rhai patches in a terminal editor, hear them play through Kira,
and expose the buffer over MCP so an AI agent (e.g. Claude Code) can
collaborate on the same script you're looking at.

The patches you design here can be embedded directly into shipped Rust
binaries via the `sndlab-core` library crate. There is no porting tax:
the same script that designs the sound plays it in the final game.

## Status

Early development. The workspace compiles; the TUI shell renders.
Engine, project model, mix model, and MCP server are landing in
order — see `book/src/SUMMARY.md` for the roadmap of chapters and
the corresponding implementation tasks.

## Quick start

```sh
# Linux only — install ALSA dev headers if you don't have them:
#   Debian/Ubuntu:  sudo apt install libasound2-dev pkg-config
#   Fedora:         sudo dnf install alsa-lib-devel pkgconf
#   Arch:           sudo pacman -S alsa-lib pkgconf

cargo run -p sndlab
```

That drops you into the TUI: editor pane on top, status bar in the
middle, log pane at the bottom. Hotkeys today:

| Key | Action |
|---|---|
| `Ctrl+R` | Evaluate the buffer (engine wiring in progress) |
| `Ctrl+S` | Save (project layer in progress) |
| `Ctrl+Q` | Quit |
| any other | Editor input |

## Workspace layout

```
sndlab/
├── crates/
│   ├── sndlab-core/   # library: rhai engine + kira/fundsp backend
│   └── sndlab/        # binary: TUI + MCP + project model
├── book/              # mdBook source for the user/DSL documentation
├── examples/          # example projects (coming with task 12)
├── LICENSE            # MIT
├── README.md
└── CLAUDE.md          # discipline for AI-assisted development
```

The split is deliberate: `sndlab-core` is reusable in any Rust
program that wants to load `.rhai` patches at runtime. The TUI is one
consumer; an embedded game audio engine is another (see
[Echo Field integration](./book/src/embedding/echo-field.md)).

## DSL at a glance

> Each primitive ships with its own chapter under `book/src/dsl/`. The
> table here is a quick reference; the chapters are authoritative.

| Primitive | Status | Sketch |
|---|---|---|
| `patch(name, role, body)` | planned | Register a named patch |
| `sine(freq_hz, dur_s)` | planned | Sine oscillator |
| `noise(kind, dur_s)` | planned | Noise generator (white/pink/brown) |
| `env(attack_s, decay_s)` | planned | Attack + exp decay envelope |
| `gain(linear)` | planned | Linear amplitude scaling |
| `mix([sig, ...])` | planned | Sum signals |
| `tap(delay_s, gain)` | planned | Delay tap (for reverb tails) |

`Status` flips to `shipped` as primitives land. Whenever a row flips,
the [DSL overview](./book/src/dsl/overview.md) status table and the
corresponding chapter both get updated in the same commit.

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
