//! Audio decoding, speech detection and transcripts.
//!
//! Retrieval over audio wants the same thing retrieval over text wants: a chunk
//! that knows where it came from. Audio is therefore decoded (pure Rust, no
//! system codecs), measured, and transcribed through a registered
//! [`Transcriber`]; every transcript segment then becomes a document carrying
//! `start_time_s`/`end_time_s`, the same contract subtitle tracks use, so time
//! windows, citation spans and the packer all work unchanged.
//!
//! What the core owns is the container and the coordinates; the speech model does
//! not belong here. `chunkr-audio` (or any Rust crate / Python plugin) registers
//! a transcriber — Whisper, a hosted API, an in-house model — mirroring how PDF
//! parsers and table extractors are plugged in.
//!
//! Two escape hatches keep that from becoming a dead end: a local `whisper.cpp`
//! binary is picked up from `CHUNKR_WHISPER_BIN`/`CHUNKR_WHISPER_MODEL`
//! ([`CommandTranscriber`]), and codecs the pure-Rust decoder lacks (Opus, above
//! all) decode through `CHUNKR_FFMPEG` when it is set.

use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard};

use serde_json::Value;

use crate::error::ChunkrError;
use crate::structures::document::Document;

mod command;
mod decode;
pub mod models;
mod vad;

pub use command::{register_from_env, CommandTranscriber};
pub use decode::{
    decode, decode_with_fallback, encode_wav, ffmpeg_configured, resample, DecodedAudio,
};
pub use models::{ModelSpec, MODELS};
pub use vad::{speech_spans, SpeechSpan, VadConfig};

/// Extensions the decoder accepts.
///
/// `.opus` and `.webm` are deliberately absent: symphonia has no Opus decoder, so
/// claiming them would turn a missing-codec gap into a confusing parse error.
pub const AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "flac", "ogg", "oga", "m4a", "m4b", "aac"];

pub use audio_extensions_with_ffmpeg as all_audio_extensions;
/// Extensions this build can load *only* through `CHUNKR_FFMPEG`.
pub use decode::FFMPEG_EXTENSIONS;

/// [`AUDIO_EXTENSIONS`] plus the containers only `ffmpeg` can open.
///
/// A `&'static` view so the dispatcher can answer "is this audio?" without
/// allocating; the ffmpeg half is empty unless one is configured.
pub fn audio_extensions_with_ffmpeg() -> &'static [&'static str] {
    use std::sync::OnceLock;
    static JOINED: OnceLock<Vec<&'static str>> = OnceLock::new();
    JOINED.get_or_init(|| {
        let mut all: Vec<&'static str> = AUDIO_EXTENSIONS.to_vec();
        all.extend_from_slice(FFMPEG_EXTENSIONS);
        all
    })
}

/// One timed transcript segment, as a speech model reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptSegment {
    /// Seconds from the start of the audio.
    pub start_s: f64,
    /// Seconds from the start of the audio.
    pub end_s: f64,
    /// Transcribed text.
    pub text: String,
    /// Speaker label, when the model does diarization.
    pub speaker: Option<String>,
}

impl TranscriptSegment {
    pub fn new(start_s: f64, end_s: f64, text: impl Into<String>) -> Self {
        Self {
            start_s,
            end_s,
            text: text.into(),
            speaker: None,
        }
    }

    pub fn with_speaker(mut self, speaker: impl Into<String>) -> Self {
        self.speaker = Some(speaker.into());
        self
    }
}

/// The audio a transcriber receives.
///
/// Both the decoded payload and the original bytes are offered: a model that
/// wants raw samples uses `audio`, one that shells out to `ffmpeg`/`whisper` uses
/// `bytes` (or `path`, when the caller had a real file to stream from).
pub struct AudioSource<'a> {
    /// Original encoded bytes.
    pub bytes: &'a [u8],
    /// File the bytes came from, when they came from one.
    pub path: Option<&'a Path>,
    /// Lower-case container extension (`mp3`, `wav`, …).
    pub format: &'a str,
    /// Decoded mono samples and the rate they are at.
    pub audio: &'a DecodedAudio,
}

/// A speech-to-text backend, registered by name.
pub trait Transcriber: Send + Sync {
    /// Model name recorded in metadata (`whisper-base`, `parakeet`, …).
    fn name(&self) -> String;

    /// Transcribe the audio into timed segments, in order.
    fn transcribe(&self, source: &AudioSource<'_>) -> Result<Vec<TranscriptSegment>, ChunkrError>;
}

type Registry = RwLock<Vec<(String, Arc<dyn Transcriber>)>>;

fn registry() -> &'static Registry {
    static TRANSCRIBERS: OnceLock<Registry> = OnceLock::new();
    TRANSCRIBERS.get_or_init(|| RwLock::new(Vec::new()))
}

// A poisoned lock cannot happen (no code panics while holding it) and would not
// invalidate the registrations anyway, so recover the guard instead of failing.
fn read_registry() -> RwLockReadGuard<'static, Vec<(String, Arc<dyn Transcriber>)>> {
    registry().read().unwrap_or_else(|error| error.into_inner())
}

fn write_registry() -> RwLockWriteGuard<'static, Vec<(String, Arc<dyn Transcriber>)>> {
    registry()
        .write()
        .unwrap_or_else(|error| error.into_inner())
}

/// Register a transcriber under `name`; registering the same name replaces it.
pub fn register_transcriber(name: &str, transcriber: Box<dyn Transcriber>) {
    let mut transcribers = write_registry();
    transcribers.retain(|(existing, _)| existing != name);
    transcribers.push((name.to_string(), Arc::from(transcriber)));
}

/// Remove a registered transcriber. Returns `true` when one was removed.
pub fn unregister_transcriber(name: &str) -> bool {
    let mut transcribers = write_registry();
    let before = transcribers.len();
    transcribers.retain(|(existing, _)| existing != name);
    transcribers.len() != before
}

/// Names of every registered transcriber, in registration order.
pub fn registered_transcribers() -> Vec<String> {
    read_registry()
        .iter()
        .map(|(name, _)| name.clone())
        .collect()
}

/// Pick the transcriber for a load.
///
/// `requested` names one explicitly; otherwise `CHUNKR_TRANSCRIBER` decides. The
/// fallback is the **most recent** registration: plugins register on `import
/// chunkr`, so a caller's own `register_transcriber` afterwards takes precedence
/// over an installed plugin without extra configuration.
fn select(requested: Option<&str>) -> Result<Arc<dyn Transcriber>, ChunkrError> {
    // A local whisper.cpp install is worth using without writing Rust; the
    // registration is env-driven and only happens when nothing claimed the name.
    command::register_from_env();

    let transcribers = read_registry();
    if transcribers.is_empty() {
        return Err(ChunkrError::ParseError(
            "audio: no transcriber registered — install `chunkr-audio` \
             (faster-whisper), set CHUNKR_WHISPER_BIN and CHUNKR_WHISPER_MODEL to \
             use a local whisper.cpp, or register one with \
             `chunkr.register_transcriber(name, fn)`"
                .to_string(),
        ));
    }

    let wanted = requested
        .map(str::to_string)
        .or_else(|| std::env::var("CHUNKR_TRANSCRIBER").ok())
        .filter(|name| !name.is_empty());

    match wanted {
        Some(name) => transcribers
            .iter()
            .find(|(existing, _)| *existing == name)
            .map(|(_, transcriber)| Arc::clone(transcriber))
            .ok_or_else(|| {
                let available: Vec<&str> =
                    transcribers.iter().map(|(name, _)| name.as_str()).collect();
                ChunkrError::ParseError(format!(
                    "audio: transcriber '{name}' is not registered (available: {})",
                    available.join(", ")
                ))
            }),
        None => Ok(Arc::clone(&transcribers.last().expect("non-empty").1)),
    }
}

/// Load audio into one `Document` per transcript segment.
///
/// Each document carries the segment span, so chunking merges them into time
/// windows that never cut a sentence in half and can be cited back to a timestamp.
pub fn load_documents(
    bytes: &[u8],
    extension: &str,
    source: Option<&str>,
    path: Option<&Path>,
) -> Result<Vec<Document>, ChunkrError> {
    load_documents_with(bytes, extension, source, path, None)
}

/// [`load_documents`] with an explicit transcriber name.
///
/// Naming one is what `CHUNKR_TRANSCRIBER` does for the dispatcher; callers that
/// want a specific model without touching process-wide state use this.
pub fn load_documents_with(
    bytes: &[u8],
    extension: &str,
    source: Option<&str>,
    path: Option<&Path>,
    requested: Option<&str>,
) -> Result<Vec<Document>, ChunkrError> {
    let format = extension.trim_start_matches('.').to_ascii_lowercase();
    let label = source.unwrap_or(&format);
    let audio = decode_with_fallback(bytes, &format)
        .map_err(|error| ChunkrError::ParseError(format!("audio {label}: {error}")))?;

    let config = VadConfig::default();
    let spans = speech_spans(&audio.samples, audio.sample_rate, &config);
    if spans.is_empty() {
        return Err(ChunkrError::ParseError(format!(
            "audio {label}: no speech detected in {}s of audio (energy VAD)",
            audio.duration_s as u64
        )));
    }

    let transcriber = select(requested)?;
    let transcribe_source = AudioSource {
        bytes,
        path,
        format: &format,
        audio: &audio,
    };
    let mut segments = transcriber.transcribe(&transcribe_source)?;
    // Models pad their last window and may report an end past the media (whisper
    // pads to 30 s and stops at the next frame boundary). A chunk citing a
    // timestamp the file does not have is worse than a shortened span, so clamp —
    // and drop anything that only exists beyond the end.
    let duration_s = audio.duration_s;
    for segment in &mut segments {
        segment.start_s = segment.start_s.clamp(0.0, duration_s);
        segment.end_s = segment.end_s.clamp(segment.start_s, duration_s);
    }
    segments.retain(|segment| segment.end_s > segment.start_s);
    if segments.is_empty() {
        return Err(ChunkrError::ParseError(format!(
            "audio {label}: transcriber '{}' returned no segments",
            transcriber.name()
        )));
    }

    let speech_s: f64 = spans.iter().map(|span| span.end_s - span.start_s).sum();
    let total_segments = segments.len();
    let mut documents = Vec::with_capacity(total_segments);

    for (index, segment) in segments.into_iter().enumerate() {
        let mut metadata = crate::loader::base::source_metadata(source);
        metadata.insert("format".to_string(), Value::from("audio"));
        metadata.insert("audio_format".to_string(), Value::from(format.clone()));
        metadata.insert("transcriber".to_string(), Value::from(transcriber.name()));
        metadata.insert(
            "audio_duration_s".to_string(),
            Value::from(audio.duration_s),
        );
        metadata.insert("sample_rate".to_string(), Value::from(audio.sample_rate));
        metadata.insert("channels".to_string(), Value::from(audio.channels));
        metadata.insert("speech_s".to_string(), Value::from(speech_s));
        metadata.insert(
            "speech_ratio".to_string(),
            Value::from((speech_s / audio.duration_s.max(f64::EPSILON)).min(1.0)),
        );
        metadata.insert("segment_index".to_string(), Value::from(index));
        metadata.insert("total_segments".to_string(), Value::from(total_segments));
        metadata.insert("start_time_s".to_string(), Value::from(segment.start_s));
        metadata.insert("end_time_s".to_string(), Value::from(segment.end_s));
        metadata.insert(
            "duration_s".to_string(),
            Value::from((segment.end_s - segment.start_s).max(0.0)),
        );
        if let Some(speaker) = segment.speaker {
            metadata.insert("speaker".to_string(), Value::from(speaker));
        }
        documents.push(Document::new(segment.text, metadata));
    }

    Ok(documents)
}
