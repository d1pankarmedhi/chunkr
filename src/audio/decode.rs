//! Container decoding: any supported audio file to mono `f32` samples.
//!
//! Pure Rust through symphonia — no system codecs, no C dependency — which keeps
//! the crate buildable everywhere the rest of the loaders are. Decoding is needed
//! for the duration, the speech measurement and for models that accept raw
//! samples; models that want the encoded file still get the original bytes.

use std::io::Cursor;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Decoded audio, downmixed to mono.
#[derive(Debug, Clone)]
pub struct DecodedAudio {
    /// Mono samples in `[-1.0, 1.0]`.
    pub samples: Vec<f32>,
    /// Sample rate of `samples`, as stored in the container.
    pub sample_rate: u32,
    /// Channel count of the source, before downmixing.
    pub channels: u16,
    /// Duration in seconds.
    pub duration_s: f64,
}

impl DecodedAudio {
    /// Peak absolute sample, useful for spotting digital silence.
    pub fn peak(&self) -> f32 {
        self.samples
            .iter()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()))
    }
}

/// Decode `bytes` to mono samples.
///
/// `format` is the container extension (`mp3`, `wav`, …) and only acts as a hint:
/// symphonia still sniffs the stream, so a mislabelled file decodes when its
/// content is recognisable.
pub fn decode(bytes: &[u8], format: &str) -> Result<DecodedAudio, String> {
    let stream = MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());

    let mut hint = Hint::new();
    if !format.is_empty() {
        hint.with_extension(format);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions {
                // Keep decoding after a damaged frame: a podcast with one bad
                // packet should still transcribe.
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(|error| format!("unsupported or corrupt audio: {error}"))?;

    let mut container = probed.format;
    let track = container
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| "no decodable audio track".to_string())?;

    let track_id = track.id;
    let channels = track
        .codec_params
        .channels
        .map(|channels| channels.count() as u16)
        .unwrap_or(1);
    let declared_rate = track.codec_params.sample_rate;
    let declared_frames = track.codec_params.n_frames;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|error| format!("unsupported audio codec: {error}"))?;

    let mut samples: Vec<f32> = Vec::new();
    let mut sample_rate = declared_rate.unwrap_or(0);
    let mut scratch: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match container.next_packet() {
            Ok(packet) => packet,
            // A truncated download ends the stream rather than failing it.
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(SymphoniaError::ResetRequired) => break,
            Err(error) => return Err(format!("audio decode failed: {error}")),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // Damage in one frame: skip it, keep the rest of the file.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(error) => return Err(format!("audio decode failed: {error}")),
        };

        if sample_rate == 0 {
            sample_rate = decoded.spec().rate;
        }
        let channel_count = decoded.spec().channels.count();
        if channel_count == 0 {
            continue;
        }

        let buffer = scratch.get_or_insert_with(|| {
            SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec())
        });
        buffer.copy_interleaved_ref(decoded);

        // Downmix by averaging: speech is mono anyway and every model expects it.
        for frame in buffer.samples().chunks(channel_count) {
            let sum: f32 = frame.iter().sum();
            samples.push(sum / channel_count as f32);
        }
    }

    if samples.is_empty() {
        return Err("no audio samples decoded".to_string());
    }

    let duration_s = match (declared_frames, sample_rate) {
        (Some(frames), rate) if rate > 0 => frames as f64 / rate as f64,
        _ => samples.len() as f64 / sample_rate.max(1) as f64,
    };

    Ok(DecodedAudio {
        samples,
        sample_rate: sample_rate.max(1),
        channels: channels.max(1),
        duration_s,
    })
}

/// Encode mono `f32` samples as a 16-bit PCM WAV file.
///
/// Used by tests and by transcriptions of generated fixtures; kept public because
/// a caller that already has samples (a VAD pass, a split) may want to hand them
/// to a transcriber that takes a file.
pub fn encode_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}
