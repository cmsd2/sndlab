# sonar-ping example

A subtractive-synthesis sonar ping designed against a recorded
reference sample. Two narrow bandpasses carve resonant peaks at
1 kHz and 2 kHz out of white noise — the noise inside each
passband supplies the "jagged" spectral texture characteristic of
real-world struck/resonant bodies. A gentle 4 Hz tremolo adds life,
a low-pass at 6 kHz tames residual high-frequency breath, and a
cosine-squared `fade_out` catches the envelope's tail so the buffer
ends at exact silence.

## How to use

Open `patches.rhai` in sndlab and press F5 to evaluate and play.
The DSL primitives in use are documented at
<https://cmsd2.github.io/sndlab/>.

## Design notes

- The third bandpass (a broad shoulder around 3.2 kHz) was tried and
  commented out — the lowpass at 6 kHz makes it redundant for this
  reference.
- Gains of 60 and 20 look extreme for a per-layer gain, but a
  narrow-Q biquad bandpass has very low output energy by design (the
  constant-skirt-gain formulation puts peak gain at Q while the
  skirts go to zero), so these high gains restore audible level
  without clipping.
- The `tremolo(4, 0.2)` is a subtle 20 % depth wobble — strong
  enough to feel alive, subtle enough to read as character rather
  than effect.
- `fade_out(0.1)` is short because the body envelope (decay 1.1)
  has already decayed substantially by the buffer end; the fade
  just guarantees the boundary lands at exactly zero.
