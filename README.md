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
scope works, patches play through Kira, and the MCP server is up so
an AI agent can edit the buffer in lockstep with you. Project model
and mix model are next — see `book/src/SUMMARY.md` for the roadmap
of chapters and the corresponding implementation tasks.

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
| `chirp(start_hz, end_hz, dur_s)` | shipped | Linear-FM sine sweep |
| `noise(kind, dur_s)` | shipped | Noise generator (white/pink/brown) |
| `signal.env(attack_s, decay_s)` | shipped | Attack + exp decay envelope |
| `signal.fade_out(duration_s)` | shipped | Cosine-squared tail fade |
| `signal.tremolo(rate_hz, depth)` | shipped | Sine-LFO amplitude modulation |
| `signal.gain(linear)` | shipped | Linear amplitude scaling |
| `signal.bandpass(center_hz, q)` | shipped | Biquad resonant bandpass filter |
| `signal.lowpass(cutoff_hz, q)` | shipped | Biquad lowpass (cuts above cutoff) |
| `signal.highpass(cutoff_hz, q)` | shipped | Biquad highpass (cuts below cutoff) |
| `mix([sig, ...])` | shipped | Sum signals |
| `tap(delay_s, gain[, decay_k])` + `signal.with_taps([...])` | shipped | Discrete reflection-style reverb (per-tap decay) |

`Status` flips to `shipped` as primitives land. Whenever a row flips,
the [DSL overview](./book/src/dsl/overview.md) status table and the
corresponding chapter both get updated in the same commit, per the
discipline in [CLAUDE.md](./CLAUDE.md).

## Comparing against a reference

The toolbar's **Load reference...** button opens an audio file
(MP3 / WAV / Ogg / FLAC) via the OS file picker. The decoded waveform
and FFT are overlaid on the scope in amber, with the patch's own
waveform and FFT in green/blue on top. Both spectra share the same
dB-reference floor so the comparison reflects relative loudness.

The typical workflow: load a reference sample, eval the patch, tune
the additive recipe until the patch's spectrum lines up with the
reference's. "Clear" removes the reference and returns to the
patch-only view.

## MCP integration

While sndlab is running, register the MCP server with Claude Code:

```sh
claude mcp add sndlab http://127.0.0.1:7777/mcp
```

The agent can then read the editor buffer, propose edits, re-evaluate,
play patches by name, and read the last error. The user sees every
edit land in the editor pane in real time. See [the MCP
chapter](./book/src/getting-started/mcp.md) for the full tool list
and the typical iteration loop.

## Documentation

Local build:

```sh
cargo install mdbook
mdbook serve book
```

Then browse <http://localhost:3000>. Online build target lives on
GitHub Pages (configuration coming with task 12).

The book is the authoritative DSL reference; this README is a tour.

## Troubleshooting

### A faint click at the start of each playback

If you're listening through laptop speakers, you may hear a brief click
at the start of each patch. This is almost always your laptop's built-in
audio amplifier coming out of a low-power state — many laptop speaker
amps have automatic gain control that ducks during silence and clicks
when "real" signal arrives. The signal coming out of sndlab is smooth;
the click is added by the hardware.

Test by plugging in headphones or external speakers. If the click
disappears, it's your laptop amp's AGC. If it persists, the OS audio
backend may be doing the same thing — try disabling any "audio
enhancement" / "smart sound" features in your system settings.

## License

MIT — see [LICENSE](LICENSE).
