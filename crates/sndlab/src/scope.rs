//! Waveform scope widget. Renders the last-played buffer as a
//! peak-to-peak vertical line per pixel column — the classic "what
//! does this sound look like" oscilloscope view, downsampled to fit
//! the available width.
//!
//! This is the primary motivation for the eframe+egui pivot: in a
//! terminal we could only approximate this with Unicode box-drawing
//! characters; here we get smooth lines at any resolution.

use egui::{Align2, Color32, FontId, Pos2, Sense, Stroke, Ui};
use sndlab_core::Buffer;

/// Colours tuned against the dark background — bright phosphor-green
/// for the waveform, subtle grid, dim foreground for the axis label.
const COLOR_BG: Color32 = Color32::from_rgb(10, 18, 14);
const COLOR_WAVE: Color32 = Color32::from_rgb(140, 230, 180);
const COLOR_AXIS: Color32 = Color32::from_rgb(40, 70, 55);
const COLOR_LABEL: Color32 = Color32::from_rgb(120, 140, 130);

/// Draw the waveform of `buffer` into the remaining ui area. If
/// `buffer` is `None`, draw an empty scope with a placeholder.
pub fn show(ui: &mut Ui, buffer: Option<&Buffer>) {
    let size = ui.available_size();
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 0.0, COLOR_BG);

    // Zero-axis line.
    let center_y = rect.center().y;
    painter.line_segment(
        [Pos2::new(rect.left(), center_y), Pos2::new(rect.right(), center_y)],
        Stroke::new(1.0, COLOR_AXIS),
    );

    let Some(buf) = buffer else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "no buffer rendered yet — F5 to eval",
            FontId::monospace(12.0),
            COLOR_LABEL,
        );
        return;
    };
    if buf.samples.is_empty() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "(empty buffer)",
            FontId::monospace(12.0),
            COLOR_LABEL,
        );
        return;
    }

    // Downsample: one pixel column per output point, each showing
    // the min..max range of the underlying sample window. This is
    // the canonical scope render for buffers much longer than the
    // pixel count.
    let n_columns = rect.width() as usize;
    if n_columns == 0 {
        return;
    }
    let samples = buf.samples.as_ref();
    let amp_scale = rect.height() * 0.45;

    for i in 0..n_columns {
        let start = i * samples.len() / n_columns;
        let end = ((i + 1) * samples.len() / n_columns).max(start + 1);
        let window = &samples[start..end.min(samples.len())];
        if window.is_empty() {
            continue;
        }
        let (lo, hi) = window.iter().fold((0.0_f32, 0.0_f32), |(a, b), s| {
            (a.min(*s), b.max(*s))
        });
        let x = rect.left() + i as f32 + 0.5;
        let y_lo = center_y - lo * amp_scale;
        let y_hi = center_y - hi * amp_scale;
        // A 1-pixel-thick vertical segment per column reads as a
        // smooth envelope at the buffer's actual resolution.
        painter.line_segment(
            [Pos2::new(x, y_hi), Pos2::new(x, y_lo)],
            Stroke::new(1.0, COLOR_WAVE),
        );
    }

    // Footer: duration + sample rate.
    let info = format!(
        "{:.2} s · {} samples · {} Hz",
        buf.samples.len() as f32 / buf.sample_rate as f32,
        buf.samples.len(),
        buf.sample_rate,
    );
    painter.text(
        Pos2::new(rect.right() - 6.0, rect.bottom() - 6.0),
        Align2::RIGHT_BOTTOM,
        info,
        FontId::monospace(11.0),
        COLOR_LABEL,
    );
}
