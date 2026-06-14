//! Decode an audio file (MP3 / WAV / Ogg / FLAC) into a mono PCM
//! buffer. Used by the `sample()` DSL primitive at patch-registration
//! time, and re-exported for the binary's reference-audio loader so
//! both paths share one decoder.

use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use symphonia::core::audio::AudioBufferRef;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::{get_codecs, get_probe};

use crate::Buffer;

#[derive(Debug)]
pub enum DecodeError {
    Io(std::io::Error),
    Symphonia(String),
    NoTrack,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "file open: {e}"),
            Self::Symphonia(s) => write!(f, "symphonia: {s}"),
            Self::NoTrack => write!(f, "no default audio track in file"),
        }
    }
}

impl std::error::Error for DecodeError {}

impl From<std::io::Error> for DecodeError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<SymphoniaError> for DecodeError {
    fn from(value: SymphoniaError) -> Self {
        Self::Symphonia(value.to_string())
    }
}

/// Decode an audio file at `path` to a mono `Buffer`. Multichannel
/// sources are averaged to mono. Sample rate is whatever the source
/// supplies; the streaming runtime resamples on tick if the source
/// rate differs from the engine's 48 kHz.
pub fn decode_file(path: &Path) -> Result<Buffer, DecodeError> {
    let file = File::open(path)?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());

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

    let track = format.default_track().ok_or(DecodeError::NoTrack)?.clone();
    let sample_rate = track.codec_params.sample_rate.unwrap_or(48_000);
    let track_id = track.id;

    let mut decoder = get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

    let mut mono: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
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
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        }
    }

    Ok(Buffer {
        sample_rate,
        samples: Arc::from(mono.into_boxed_slice()),
    })
}

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
