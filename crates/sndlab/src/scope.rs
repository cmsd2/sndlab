//! Scope widget. Split vertically:
//!
//! - Top half: time-domain peak-to-peak waveform of the last rendered
//!   buffer.
//! - Bottom half: magnitude spectrum (FFT) of the same buffer, with
//!   linear frequency axis and dB-scaled magnitude.
//!
//! This is the primary motivation for the eframe+egui pivot: in a
//! terminal we could only approximate this with Unicode box-drawing
//! characters; here we get smooth lines at any resolution and the
//! spectral view shows the chirp's bandwidth at a glance.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use sndlab_core::Buffer;

const COLOR_BG: Color32 = Color32::from_rgb(10, 18, 14);
const COLOR_WAVE: Color32 = Color32::from_rgb(140, 230, 180);
const COLOR_SPEC: Color32 = Color32::from_rgb(180, 200, 240);
// Amber for reference, distinct from the green patch wave and the
// blue patch spectrum.
const COLOR_REF_WAVE: Color32 = Color32::from_rgb(240, 180, 100);
const COLOR_REF_SPEC: Color32 = Color32::from_rgb(240, 150, 80);
const COLOR_AXIS: Color32 = Color32::from_rgb(40, 70, 55);
const COLOR_LABEL: Color32 = Color32::from_rgb(120, 140, 130);

/// Upper limit of the spectrum display. 6 kHz is wide enough to show
/// everything the DSL realistically generates (sonar pings, hull
/// creaks, mid-range ambience) without wasting pixels on empty bins.
const SPECTRUM_MAX_HZ: f32 = 6_000.0;
/// dB range shown vertically in the spectrum. -80 dB is below any
/// useful signal; 0 dB is at the top of the panel.
const SPECTRUM_FLOOR_DB: f32 = -80.0;

/// Draw the split scope into `ui`. The waveform and spectrum each get
/// half the vertical space. Reference buffer/spectrum (if provided)
/// are drawn on top of the patch in a contrasting amber colour.
pub fn show(
    ui: &mut Ui,
    buffer: Option<&Buffer>,
    spectrum: Option<&[f32]>,
    reference: Option<&Buffer>,
    reference_spectrum: Option<&[f32]>,
) {
    let size = ui.available_size();
    let half_h = (size.y * 0.5).floor();
    let wave_size = Vec2::new(size.x, half_h);
    let spec_size = Vec2::new(size.x, size.y - half_h);

    let (wave_rect, _) = ui.allocate_exact_size(wave_size, Sense::hover());
    let (spec_rect, _) = ui.allocate_exact_size(spec_size, Sense::hover());

    draw_waveform(ui, wave_rect, buffer, reference);
    draw_spectrum(ui, spec_rect, buffer, spectrum, reference, reference_spectrum);
}

fn draw_waveform(ui: &Ui, rect: Rect, buffer: Option<&Buffer>, reference: Option<&Buffer>) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, COLOR_BG);

    let center_y = rect.center().y;
    painter.line_segment(
        [
            Pos2::new(rect.left(), center_y),
            Pos2::new(rect.right(), center_y),
        ],
        Stroke::new(1.0, COLOR_AXIS),
    );

    // If neither buffer nor reference is present, prompt and return.
    if buffer.is_none() && reference.is_none() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "no buffer rendered yet — F5 to eval",
            FontId::monospace(12.0),
            COLOR_LABEL,
        );
        return;
    }

    let n_columns = rect.width() as usize;
    if n_columns == 0 {
        return;
    }
    let amp_scale = rect.height() * 0.45;

    // Reference drawn first so the patch's wave sits on top.
    if let Some(buf) = reference.filter(|b| !b.samples.is_empty()) {
        draw_peaks(&painter, rect, center_y, amp_scale, &buf.samples, COLOR_REF_WAVE);
    }
    if let Some(buf) = buffer.filter(|b| !b.samples.is_empty()) {
        draw_peaks(&painter, rect, center_y, amp_scale, &buf.samples, COLOR_WAVE);
    }

    // Footer info — show the patch's info if present, else the
    // reference's. If both are present they line up over the same
    // time axis only if you treat each as its own time scale, so
    // we list both lengths so the user can read the comparison.
    let info = match (buffer, reference) {
        (Some(p), Some(r)) => format!(
            "patch {:.2}s · ref {:.2}s",
            p.samples.len() as f32 / p.sample_rate as f32,
            r.samples.len() as f32 / r.sample_rate as f32,
        ),
        (Some(b), None) | (None, Some(b)) => format!(
            "{:.2} s · {} samples · {} Hz",
            b.samples.len() as f32 / b.sample_rate as f32,
            b.samples.len(),
            b.sample_rate,
        ),
        (None, None) => String::new(),
    };
    if !info.is_empty() {
        painter.text(
            Pos2::new(rect.right() - 6.0, rect.bottom() - 6.0),
            Align2::RIGHT_BOTTOM,
            info,
            FontId::monospace(11.0),
            COLOR_LABEL,
        );
    }
}

/// Draw min/max peaks of a sample buffer across the rectangle.
fn draw_peaks(
    painter: &egui::Painter,
    rect: Rect,
    center_y: f32,
    amp_scale: f32,
    samples: &[f32],
    colour: Color32,
) {
    let n_columns = rect.width() as usize;
    let stroke = Stroke::new(1.0, colour);
    for i in 0..n_columns {
        let start = i * samples.len() / n_columns;
        let end = ((i + 1) * samples.len() / n_columns).max(start + 1);
        let window = &samples[start..end.min(samples.len())];
        if window.is_empty() {
            continue;
        }
        let (lo, hi) = window
            .iter()
            .fold((0.0_f32, 0.0_f32), |(a, b), s| (a.min(*s), b.max(*s)));
        let x = rect.left() + i as f32 + 0.5;
        let y_lo = center_y - lo * amp_scale;
        let y_hi = center_y - hi * amp_scale;
        painter.line_segment([Pos2::new(x, y_hi), Pos2::new(x, y_lo)], stroke);
    }
}

fn draw_spectrum(
    ui: &Ui,
    rect: Rect,
    buffer: Option<&Buffer>,
    spectrum: Option<&[f32]>,
    reference: Option<&Buffer>,
    reference_spectrum: Option<&[f32]>,
) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, COLOR_BG);

    // Top border so the eye reads two stacked panels even at small
    // sizes.
    painter.line_segment(
        [
            Pos2::new(rect.left(), rect.top()),
            Pos2::new(rect.right(), rect.top()),
        ],
        Stroke::new(1.0, COLOR_AXIS),
    );

    if spectrum.is_none() && reference_spectrum.is_none() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "spectrum appears after F5",
            FontId::monospace(12.0),
            COLOR_LABEL,
        );
        return;
    }

    // Frequency-axis grid lines + labels: every 1 kHz from 0 to the
    // spectrum's upper limit.
    let plot_h = rect.height();
    for f in (0..=(SPECTRUM_MAX_HZ as i32)).step_by(1_000) {
        let x = rect.left() + (f as f32 / SPECTRUM_MAX_HZ) * rect.width();
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(0.5, COLOR_AXIS),
        );
        if f > 0 && (f % 2_000) == 0 {
            painter.text(
                Pos2::new(x + 2.0, rect.bottom() - 2.0),
                Align2::LEFT_BOTTOM,
                format!("{} kHz", f / 1_000),
                FontId::monospace(10.0),
                COLOR_LABEL,
            );
        }
    }

    // The 0-dB reference is the max magnitude across both spectra,
    // so direct visual comparison reflects relative loudness.
    let max_patch = bin_max_in_band(buffer, spectrum);
    let max_ref = bin_max_in_band(reference, reference_spectrum);
    let max_mag = max_patch.max(max_ref).max(1e-12);

    // Reference first so the patch overlay sits on top.
    if let (Some(buf), Some(spec)) = (reference, reference_spectrum) {
        draw_spectrum_bars(&painter, rect, plot_h, buf, spec, max_mag, COLOR_REF_SPEC);
    }
    if let (Some(buf), Some(spec)) = (buffer, spectrum) {
        draw_spectrum_bars(&painter, rect, plot_h, buf, spec, max_mag, COLOR_SPEC);
    }

    painter.text(
        Pos2::new(rect.right() - 6.0, rect.top() + 4.0),
        Align2::RIGHT_TOP,
        format!(
            "spectrum · 0 – {:.0} kHz · {:.0} dB floor",
            SPECTRUM_MAX_HZ / 1000.0,
            SPECTRUM_FLOOR_DB
        ),
        FontId::monospace(10.0),
        COLOR_LABEL,
    );
}

/// Maximum magnitude in the visible (0 → SPECTRUM_MAX_HZ) band of one
/// spectrum, or 0.0 if either input is missing. Used to set the
/// shared dB-reference floor.
fn bin_max_in_band(buffer: Option<&Buffer>, spectrum: Option<&[f32]>) -> f32 {
    let (Some(buf), Some(spec)) = (buffer, spectrum) else {
        return 0.0;
    };
    if spec.is_empty() {
        return 0.0;
    }
    let nyquist = buf.sample_rate as f32 * 0.5;
    let bins_per_hz = spec.len() as f32 / nyquist;
    let max_bin = (SPECTRUM_MAX_HZ * bins_per_hz).min(spec.len() as f32) as usize;
    spec[..max_bin].iter().copied().fold(0.0_f32, f32::max)
}

fn draw_spectrum_bars(
    painter: &egui::Painter,
    rect: Rect,
    plot_h: f32,
    buf: &Buffer,
    spec: &[f32],
    max_mag: f32,
    colour: Color32,
) {
    if spec.is_empty() {
        return;
    }
    let nyquist = buf.sample_rate as f32 * 0.5;
    let bins_per_hz = spec.len() as f32 / nyquist;
    let max_bin = (SPECTRUM_MAX_HZ * bins_per_hz).min(spec.len() as f32) as usize;
    if max_bin == 0 {
        return;
    }
    let n_columns = rect.width() as usize;
    if n_columns == 0 {
        return;
    }
    let stroke = Stroke::new(1.0, colour);
    for i in 0..n_columns {
        let start_bin = i * max_bin / n_columns;
        let end_bin = ((i + 1) * max_bin / n_columns).max(start_bin + 1);
        let window = &spec[start_bin..end_bin.min(spec.len())];
        if window.is_empty() {
            continue;
        }
        let bin_max = window.iter().copied().fold(0.0_f32, f32::max);
        if bin_max <= 0.0 {
            continue;
        }
        let db = 20.0 * (bin_max / max_mag).log10();
        let normalised = ((db - SPECTRUM_FLOOR_DB) / -SPECTRUM_FLOOR_DB).clamp(0.0, 1.0);
        let h = normalised * plot_h;
        let x = rect.left() + i as f32 + 0.5;
        painter.line_segment(
            [Pos2::new(x, rect.bottom()), Pos2::new(x, rect.bottom() - h)],
            stroke,
        );
    }
}
