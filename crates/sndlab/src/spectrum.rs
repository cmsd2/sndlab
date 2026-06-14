//! FFT-based spectrum analysis for the scope's lower pane.
//!
//! `compute(buffer)` runs a single forward FFT over the buffer and
//! returns the per-bin magnitudes (Nyquist-limited — we drop the
//! mirrored upper half). The result is cached on the App as
//! `last_spectrum`; the scope reads it each frame to render bars.

use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;
use sndlab_core::Buffer;

/// Largest FFT we'll run, in samples. 16384 at 48 kHz gives ~3 Hz
/// bin resolution over the spectrum — fine grain for low-frequency
/// content. Bigger buffers get downsampled by taking the leading
/// 16k samples; that's representative of any patch's onset.
const MAX_FFT_SIZE: usize = 16_384;

/// Compute the magnitude spectrum of `buffer`. Returns half the FFT
/// (positive frequencies only), one value per bin.
pub fn compute(buffer: &Buffer) -> Vec<f32> {
    if buffer.samples.is_empty() {
        return Vec::new();
    }
    let n = buffer.samples.len().min(MAX_FFT_SIZE);
    // Round down to a multiple of 2 so the bin count is clean. Real
    // FFTs work for any size in rustfft, but power-of-two sizes are
    // dramatically faster and we don't need exact length here.
    let n = pow2_le(n);
    // A 1-sample buffer rounds down to a power-of-two of 1, which is
    // a degenerate FFT and would also slice past `buffer.samples` if
    // we forced a larger n. Drop short buffers — the scope shows the
    // "no buffer" placeholder, which is the right UX anyway.
    if n < 2 {
        return Vec::new();
    }
    let mut buf: Vec<Complex32> = buffer.samples[..n]
        .iter()
        .enumerate()
        .map(|(i, &s)| Complex32::new(s * window(n, i), 0.0))
        .collect();

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(n);
    fft.process(&mut buf);

    // Magnitude of the first half (positive frequencies). Scale by
    // 2/n so the result is in roughly the same range as the time-
    // domain peak (within a window-correction constant).
    let half = n / 2;
    let scale = 2.0 / n as f32;
    buf[..half]
        .iter()
        .map(|c| (c.re * c.re + c.im * c.im).sqrt() * scale)
        .collect()
}

/// Hann window — smooths the FFT's edges so a non-periodic buffer
/// doesn't smear energy across all bins.
fn window(n: usize, i: usize) -> f32 {
    let x = i as f32 / (n - 1).max(1) as f32;
    let w = (std::f32::consts::PI * x).sin();
    w * w
}

/// Largest power of two ≤ `n`.
fn pow2_le(n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let mut p = 1;
    while p * 2 <= n {
        p *= 2;
    }
    p
}
