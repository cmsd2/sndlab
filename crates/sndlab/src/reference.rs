//! Load a reference audio file (MP3 / WAV / Ogg / FLAC) and decode
//! to a mono `Buffer` for display in the scope. Powered by symphonia,
//! which Kira already pulls in transitively; we depend on it
//! directly so we can call its decoding API.
//!
//! The decoded buffer is owned by the app side (not the engine) and
//! shown on the scope alongside whatever patch buffer is current,
//! so designers can A/B their additive recipes against a recording.

use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use sndlab_core::Buffer;
use symphonia::core::audio::AudioBufferRef;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::{get_codecs, get_probe};

#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    Symphonia(String),
    NoTrack,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "file open: {e}"),
            Self::Symphonia(s) => write!(f, "symphonia: {s}"),
            Self::NoTrack => write!(f, "no default audio track in file"),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<std::io::Error> for LoadError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<SymphoniaError> for LoadError {
    fn from(value: SymphoniaError) -> Self {
        Self::Symphonia(value.to_string())
    }
}

/// Decode an audio file at `path` to a mono `Buffer`. Multichannel
/// sources are averaged to mono so the scope renders a single
/// waveform. Sample rate is whatever the source supplies.
pub fn load(path: &Path) -> Result<Buffer, LoadError> {
    let file = File::open(path)?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());

    // Hint the probe with the extension so it doesn't have to guess.
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        hint.with_extension(ext);
    }

    let probed = get_probe().format(
        &hint,
        stream,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;
    let mut format = probed.format;

    let track = format
        .default_track()
        .ok_or(LoadError::NoTrack)?
        .clone();
    let sample_rate = track.codec_params.sample_rate.unwrap_or(48_000);
    let track_id = track.id;

    let mut decoder = get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

    let mut mono: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            // Either a clean end-of-stream or a reset-required error
            // (which we treat as end-of-stream because we don't have a
            // recovery strategy for partial decodes).
            Err(SymphoniaError::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => append_mono(&decoded, &mut mono),
            // Skip a malformed packet — we'd rather get a partial
            // decode than refuse the whole file.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        }
    }

    Ok(Buffer {
        sample_rate,
        samples: Arc::from(mono.into_boxed_slice()),
    })
}

/// Append decoded samples to `out`, averaging across channels.
fn append_mono(decoded: &AudioBufferRef<'_>, out: &mut Vec<f32>) {
    match decoded {
        AudioBufferRef::F32(buf) => append_planar_f32(buf.planes().planes(), out),
        AudioBufferRef::F64(buf) => {
            let planes = buf.planes();
            let chans = planes.planes();
            for i in 0..chans[0].len() {
                let s: f64 = chans.iter().map(|c| c[i]).sum();
                out.push((s / chans.len() as f64) as f32);
            }
        }
        AudioBufferRef::S16(buf) => {
            let planes = buf.planes();
            let chans = planes.planes();
            let inv_scale = 1.0 / i16::MAX as f32;
            for i in 0..chans[0].len() {
                let s: i32 = chans.iter().map(|c| c[i] as i32).sum();
                out.push((s as f32 / chans.len() as f32) * inv_scale);
            }
        }
        AudioBufferRef::S32(buf) => {
            let planes = buf.planes();
            let chans = planes.planes();
            let inv_scale = 1.0 / i32::MAX as f32;
            for i in 0..chans[0].len() {
                let s: i64 = chans.iter().map(|c| c[i] as i64).sum();
                out.push((s as f32 / chans.len() as f32) * inv_scale);
            }
        }
        AudioBufferRef::U8(buf) => {
            let planes = buf.planes();
            let chans = planes.planes();
            for i in 0..chans[0].len() {
                let s: i32 = chans.iter().map(|c| c[i] as i32 - 128).sum();
                out.push(s as f32 / chans.len() as f32 / 128.0);
            }
        }
        // Less common formats fall through to silence so we don't
        // panic on an exotic file. A warning in the app log would be
        // appropriate but the caller is the better place to log.
        _ => {}
    }
}

fn append_planar_f32(planes: &[&[f32]], out: &mut Vec<f32>) {
    if planes.is_empty() {
        return;
    }
    let len = planes[0].len();
    let n_chans = planes.len() as f32;
    for i in 0..len {
        let s: f32 = planes.iter().map(|p| p[i]).sum();
        out.push(s / n_chans);
    }
}
