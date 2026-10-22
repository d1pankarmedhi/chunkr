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

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|error| match sniff_codec(bytes) {
            Some(codec) => format!("unsupported audio codec '{codec}' ({error})"),
            None => format!("unsupported audio codec: {error}"),
        })?;

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

    // Duration comes from the samples actually decoded, not from what the header
    // declares: a WAV piped in with an unknown data size reports ~2^32 frames
    // (12 hours for a 4-second file), and streamed or truncated containers lie in
    // the other direction. Timestamps must match the audio a model was given.
    let duration_s = samples.len() as f64 / sample_rate.max(1) as f64;

    Ok(DecodedAudio {
        samples,
        sample_rate: sample_rate.max(1),
        channels: channels.max(1),
        duration_s,
    })
}

/// Containers the built-in decoder cannot open at all, but `ffmpeg` handles.
///
/// Video is included on purpose: the speech in a talk, a lecture recording or a
/// screen share is what retrieval wants from it, and `ffmpeg` extracts that track
/// like any other audio. It is only advertised when `CHUNKR_FFMPEG` is set, so an
/// extension list never promises what this build cannot do.
pub const FFMPEG_EXTENSIONS: &[&str] = &[
    "opus", "webm", "mkv", "mp4", "m4v", "mov", "avi", "aiff", "aif", "wma", "amr", "caf",
];

/// True when an external `ffmpeg` is configured (`CHUNKR_FFMPEG`).
pub fn ffmpeg_configured() -> bool {
    matches!(std::env::var("CHUNKR_FFMPEG"), Ok(path) if !path.trim().is_empty())
}

/// Container codec names for the formats symphonia cannot decode, so the error
/// says "opus" instead of "unsupported codec".
///
/// Provenance: WhatsApp notes, browser recordings and anything downloaded with
/// `yt-dlp` are Opus far more often than Vorbis, and Opus has no decoder in
/// symphonia — the extension alone would promise something we cannot do.
fn sniff_codec(bytes: &[u8]) -> Option<&'static str> {
    let head = &bytes[..bytes.len().min(4096)];
    for (magic, name) in [
        (&b"OpusHead"[..], "opus"),
        (b"\x01vorbis", "vorbis"),
        (b"Speex   ", "speex"),
        (b"#!AMR", "amr"),
        (b"A_OPUS", "opus"),
        (b"A_VORBIS", "vorbis"),
    ] {
        if head.windows(magic.len()).any(|window| window == magic) {
            return Some(name);
        }
    }
    None
}

/// [`decode`], with an external `ffmpeg` as the fallback for codecs the built-in
/// decoder does not cover (Opus above all).
///
/// Set `CHUNKR_FFMPEG` to the binary to enable it. Nothing runs unless that is
/// set and the pure-Rust path actually failed, so the default build stays
/// dependency-free while real-world files remain usable.
pub fn decode_with_fallback(bytes: &[u8], format: &str) -> Result<DecodedAudio, String> {
    match decode(bytes, format) {
        Ok(audio) => Ok(audio),
        Err(error) => {
            let ffmpeg = match std::env::var("CHUNKR_FFMPEG") {
                Ok(path) if !path.trim().is_empty() => path.trim().to_string(),
                _ => {
                    return Err(match sniff_codec(bytes) {
                        Some(codec) => format!(
                            "{error} — this file is {codec}; install ffmpeg and set \
                             CHUNKR_FFMPEG=/path/to/ffmpeg, or convert it to wav/mp3"
                        ),
                        None => error,
                    })
                }
            };
            decode_via_ffmpeg(&ffmpeg, bytes, format)
                .map_err(|ffmpeg_error| format!("{error} — ffmpeg fallback failed: {ffmpeg_error}"))
        }
    }
}

/// Decode through an external `ffmpeg`, which handles every codec the built-in
/// decoder does not (Opus, WebM, AMR, …) and writes a WAV the built-in path reads.
fn decode_via_ffmpeg(program: &str, bytes: &[u8], format: &str) -> Result<DecodedAudio, String> {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ticket = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("chunkr-decode-{}-{ticket}", std::process::id()));
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
    let input = dir.join(format!(
        "input.{}",
        if format.is_empty() { "bin" } else { format }
    ));

    let result = (|| {
        std::fs::write(&input, bytes).map_err(|error| format!("cannot write input: {error}"))?;
        let output = std::process::Command::new(program)
            .arg("-v")
            .arg("error")
            .arg("-i")
            .arg(&input)
            .arg("-ac")
            .arg("1")
            .arg("-f")
            .arg("wav")
            .arg("pipe:1")
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output()
            .map_err(|error| format!("cannot run {program}: {error}"))?;
        if !output.status.success() || output.stdout.is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let reason = stderr.lines().last().unwrap_or("no output");
            return Err(format!("{program} exited with {}: {reason}", output.status));
        }
        decode(&output.stdout, "wav")
    })();

    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Resample to `target_rate`, band-limiting before decimating.
///
/// Speech models want 16 kHz; containers deliver whatever they were encoded at
/// (44.1 or 48 kHz for real recordings), so handing a model the raw rate either
/// fails or silently stretches the audio. Downsampling averages over the ratio
/// first — otherwise a 44.1 → 16 kHz decimation folds high frequencies back into
/// the speech band — then interpolates linearly.
pub fn resample(audio: &DecodedAudio, target_rate: u32) -> DecodedAudio {
    let rate = audio.sample_rate.max(1);
    let target_rate = target_rate.max(1);
    if rate == target_rate || audio.samples.is_empty() {
        return DecodedAudio {
            sample_rate: target_rate,
            duration_s: samples_duration(&audio.samples, target_rate),
            ..audio.clone()
        };
    }

    let ratio = rate as f64 / target_rate as f64;
    let filtered = if ratio > 1.0 {
        moving_average(&audio.samples, ratio.round().max(2.0) as usize)
    } else {
        audio.samples.clone()
    };

    let out_len = (filtered.len() as f64 / ratio).round() as usize;
    let mut samples = Vec::with_capacity(out_len);
    for index in 0..out_len {
        let position = index as f64 * ratio;
        let left = position.floor() as usize;
        let fraction = (position - left as f64) as f32;
        let a = filtered.get(left).copied().unwrap_or(0.0);
        let b = filtered.get(left + 1).copied().unwrap_or(a);
        samples.push(a + (b - a) * fraction);
    }

    DecodedAudio {
        duration_s: samples_duration(&samples, target_rate),
        samples,
        sample_rate: target_rate,
        channels: audio.channels,
    }
}

fn samples_duration(samples: &[f32], sample_rate: u32) -> f64 {
    samples.len() as f64 / sample_rate.max(1) as f64
}

/// Centered box filter of `window` taps, via a prefix sum.
fn moving_average(samples: &[f32], window: usize) -> Vec<f32> {
    if window < 2 || samples.len() < window {
        return samples.to_vec();
    }
    let half = window / 2;
    let mut prefix = Vec::with_capacity(samples.len() + 1);
    prefix.push(0.0f32);
    for sample in samples {
        prefix.push(prefix[prefix.len() - 1] + sample);
    }
    (0..samples.len())
        .map(|index| {
            let start = index.saturating_sub(half);
            let end = (index + window - half).min(samples.len());
            let count = (end - start).max(1);
            (prefix[end] - prefix[start]) / count as f32
        })
        .collect()
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
