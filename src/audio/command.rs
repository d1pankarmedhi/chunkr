//! A transcriber that shells out to a locally installed `whisper.cpp` binary.
//!
//! The core keeps no model, but a CLI user with a ggml file on disk should not
//! have to write Rust to use it. `whisper-cli` takes a 16 kHz mono WAV and writes
//! JSON, so this backend writes a temporary WAV from the samples the core already
//! decoded (no second decoder, no `ffmpeg` on the path), runs the binary, and maps
//! its segments onto [`TranscriptSegment`].

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

use crate::audio::decode::{encode_wav, resample};
use crate::audio::{AudioSource, Transcriber, TranscriptSegment};
use crate::error::ChunkrError;

/// Rate `whisper.cpp` requires; anything else is rejected by the binary.
const WHISPER_RATE: u32 = 16_000;

/// A `whisper.cpp` binary plus the ggml model it should load.
pub struct CommandTranscriber {
    program: PathBuf,
    model: PathBuf,
    threads: usize,
}

impl CommandTranscriber {
    /// Use `program` (`whisper-cli`) with `model` (`ggml-base.en.bin`).
    pub fn whisper_cpp(program: impl Into<PathBuf>, model: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            model: model.into(),
            threads: std::thread::available_parallelism()
                .map(|count| count.get().min(8))
                .unwrap_or(4),
        }
    }

    /// Read the pair from `CHUNKR_WHISPER_BIN` and `CHUNKR_WHISPER_MODEL`.
    pub fn from_env() -> Option<Self> {
        let program = std::env::var("CHUNKR_WHISPER_BIN").ok()?;
        let model = std::env::var("CHUNKR_WHISPER_MODEL").ok()?;
        if program.trim().is_empty() || model.trim().is_empty() {
            return None;
        }
        Some(Self::whisper_cpp(program.trim(), model.trim()))
    }

    fn model_name(&self) -> String {
        let stem = self
            .model
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_else(|| "model".to_string());
        format!("whisper.cpp/{stem}")
    }
}

impl Transcriber for CommandTranscriber {
    fn name(&self) -> String {
        self.model_name()
    }

    fn transcribe(&self, source: &AudioSource<'_>) -> Result<Vec<TranscriptSegment>, ChunkrError> {
        let dir = scratch_dir();
        std::fs::create_dir_all(&dir).map_err(|error| {
            ChunkrError::ParseError(format!("audio: cannot create {}: {error}", dir.display()))
        })?;
        let wav = dir.join("input.wav");
        let out = dir.join("output");

        // Always 16 kHz mono: whisper.cpp rejects other rates, and resampling here
        // (rather than asking for ffmpeg) keeps the CLI dependency-free.
        let audio = resample(source.audio, WHISPER_RATE);
        std::fs::write(&wav, encode_wav(&audio.samples, audio.sample_rate)).map_err(|error| {
            ChunkrError::ParseError(format!("audio: cannot write {}: {error}", wav.display()))
        })?;

        let output = Command::new(&self.program)
            .arg("-m")
            .arg(&self.model)
            .arg("-f")
            .arg(&wav)
            .arg("-t")
            .arg(self.threads.to_string())
            .arg("-oj")
            // `-ojf` adds token offsets; without it the JSON has no word timings at
            // all. It is ignored by whisper.cpp builds that predate the flag.
            .arg("-ojf")
            .arg("-of")
            .arg(&out)
            .arg("-np")
            .stdin(Stdio::null())
            .output();

        let result = output
            .map_err(|error| {
                ChunkrError::ParseError(format!(
                    "audio: cannot run {} ({error}) — install whisper.cpp and set \
                     CHUNKR_WHISPER_BIN, or use `chunkr-audio` from Python",
                    self.program.display()
                ))
            })
            .and_then(|output| {
                read_segments(&out.with_extension("json"), &output).inspect_err(|_| {
                    let _ = std::fs::remove_dir_all(&dir);
                })
            });

        let _ = std::fs::remove_dir_all(&dir);
        result
    }
}

/// Parse whisper.cpp's `-oj` output.
///
/// The shape is `{"transcription": [{"offsets": {"from": ms, "to": ms},
/// "text": " …", "tokens": [{"text": " I", "offsets": {...}}]}]}`; offsets are
/// milliseconds and text arrives with a leading space and any
/// `[BLANK_AUDIO]`/`(music)` markers the model emits.
///
/// Tokens are merged into words: whisper.cpp's vocabulary is word-level for common
/// words and sub-word for the rest, and a word is identified by the space its token
/// starts with. Punctuation arrives as its own token and is glued back to the word
/// before it.
fn read_segments(
    path: &Path,
    output: &std::process::Output,
) -> Result<Vec<TranscriptSegment>, ChunkrError> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = stderr
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("no output");
        return Err(ChunkrError::ParseError(format!(
            "audio: whisper.cpp exited with {}: {reason}",
            output.status
        )));
    }

    let text = std::fs::read_to_string(path).map_err(|error| {
        ChunkrError::ParseError(format!(
            "audio: whisper.cpp wrote no JSON at {} ({error})",
            path.display()
        ))
    })?;
    let json: Value = serde_json::from_str(&text).map_err(|error| {
        ChunkrError::ParseError(format!("audio: whisper.cpp JSON is malformed: {error}"))
    })?;

    let entries = json
        .get("transcription")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ChunkrError::ParseError("audio: whisper.cpp JSON has no transcription".to_string())
        })?;

    let mut segments = Vec::with_capacity(entries.len());
    for entry in entries {
        let text = entry
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        // `[BLANK_AUDIO]`, `(music)`, `[SOUND]`: markers, not speech.
        if text.is_empty() || (text.starts_with('[') && text.ends_with(']')) {
            continue;
        }
        let from = entry
            .pointer("/offsets/from")
            .and_then(Value::as_f64)
            .unwrap_or_default();
        let to = entry
            .pointer("/offsets/to")
            .and_then(Value::as_f64)
            .unwrap_or(from);
        let segment = TranscriptSegment::new(
            (from / 1000.0).max(0.0),
            (to / 1000.0).max(from / 1000.0),
            text,
        );
        let words = entry
            .get("tokens")
            .and_then(Value::as_array)
            .map(|tokens| words_from_tokens(tokens))
            .unwrap_or_default();
        segments.push(if words.is_empty() {
            segment
        } else {
            segment.with_words(words)
        });
    }
    Ok(segments)
}

/// Merge whisper.cpp tokens into words.
///
/// A token starting with a space begins a word; anything else extends the word
/// already open (sub-word pieces, trailing punctuation). Special tokens —
/// `[_BEG_]`, `[_TT_23]` — and zero-length tokens are dropped.
fn words_from_tokens(tokens: &[Value]) -> Vec<crate::audio::WordTiming> {
    let mut words: Vec<crate::audio::WordTiming> = Vec::new();
    for token in tokens {
        let text = token
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let from = token
            .pointer("/offsets/from")
            .and_then(Value::as_f64)
            .unwrap_or_default()
            / 1000.0;
        let to = token
            .pointer("/offsets/to")
            .and_then(Value::as_f64)
            .unwrap_or(from * 1000.0)
            / 1000.0;
        let stripped = text.trim();
        if stripped.is_empty() || (stripped.starts_with('[') && stripped.ends_with(']')) {
            continue;
        }
        let starts_word = text.starts_with(' ');
        match words.last_mut() {
            Some(word) if !starts_word => {
                word.text.push_str(stripped);
                word.end_s = word.end_s.max(to);
            }
            _ => words.push(crate::audio::WordTiming::new(stripped, from, to)),
        }
    }
    words.retain(|word| !word.text.is_empty());
    words
}

/// Unique per call, so two threads transcribing cannot collide.
fn scratch_dir() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ticket = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("chunkr-whisper-{}-{ticket}", std::process::id()))
}

/// Register `whisper.cpp` from the environment, unless a transcriber of that name
/// is already registered.
///
/// Called by the dispatcher, so `CHUNKR_WHISPER_BIN` + `CHUNKR_WHISPER_MODEL` are
/// all it takes for `chunkr talk.mp3` to work from the CLI.
pub fn register_from_env() {
    let Some(transcriber) = CommandTranscriber::from_env() else {
        return;
    };
    if crate::audio::registered_transcribers()
        .iter()
        .any(|name| name == "whisper-cpp")
    {
        return;
    }
    crate::audio::register_transcriber("whisper-cpp", Box::new(transcriber));
}
