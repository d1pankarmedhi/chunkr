//! Audio: decoding, speech detection, transcripts and time-window chunking.

#![cfg(all(feature = "audio", not(target_arch = "wasm32")))]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use chunkr::audio::{
    decode, encode_wav, load_documents_with, register_transcriber, registered_transcribers,
    speech_spans, unregister_transcriber, Transcriber, TranscriptSegment, VadConfig,
};
use chunkr::prelude::*;

/// A transcriber that reports fixed segments, standing in for Whisper.
struct FakeTranscriber {
    name: &'static str,
    segments: Vec<TranscriptSegment>,
    calls: Arc<AtomicUsize>,
}

impl Transcriber for FakeTranscriber {
    fn name(&self) -> String {
        self.name.to_string()
    }

    fn transcribe(
        &self,
        _source: &chunkr::audio::AudioSource<'_>,
    ) -> Result<Vec<TranscriptSegment>, ChunkrError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(self.segments.clone())
    }
}

fn tone(samples: &mut Vec<f32>, seconds: f64, rate: u32) {
    for index in 0..(seconds * rate as f64) as usize {
        let phase = 2.0 * std::f32::consts::PI * 220.0 * index as f32 / rate as f32;
        samples.push(phase.sin() * 0.3);
    }
}

fn speech_then_silence(rate: u32) -> Vec<f32> {
    let mut samples = Vec::new();
    samples.extend(std::iter::repeat_n(0.0, (0.5 * rate as f64) as usize));
    tone(&mut samples, 1.5, rate);
    samples.extend(std::iter::repeat_n(0.0, (0.5 * rate as f64) as usize));
    samples
}

fn wav_fixture() -> Vec<u8> {
    encode_wav(&speech_then_silence(16_000), 16_000)
}

const SEGMENTS: &[(&str, f64, f64, Option<&str>)] = &[
    ("Welcome to the show.", 0.5, 1.2, Some("Host")),
    ("Today we talk about chunking.", 1.2, 2.0, Some("Host")),
    ("Great to be here.", 2.0, 2.4, Some("Guest")),
];

fn fake(name: &'static str) -> Arc<AtomicUsize> {
    let calls = Arc::new(AtomicUsize::new(0));
    register_transcriber(
        name,
        Box::new(FakeTranscriber {
            name,
            segments: SEGMENTS
                .iter()
                .map(|(text, start, end, speaker)| {
                    let segment = TranscriptSegment::new(*start, *end, *text);
                    match speaker {
                        Some(speaker) => segment.with_speaker(*speaker),
                        None => segment,
                    }
                })
                .collect(),
            calls: Arc::clone(&calls),
        }),
    );
    calls
}

#[test]
fn decodes_a_wav_and_reports_its_shape() {
    let audio = decode(&wav_fixture(), "wav").unwrap();
    assert_eq!(audio.sample_rate, 16_000);
    assert_eq!(audio.channels, 1);
    assert!((audio.duration_s - 2.5).abs() < 0.05, "{audio:?}");
    assert!(audio.samples.len() >= 16_000 * 2, "{}", audio.samples.len());
    assert!(audio.peak() > 0.2, "speech survived decoding");
}

#[test]
fn speech_detection_finds_the_talked_part() {
    let audio = decode(&wav_fixture(), "wav").unwrap();
    let spans = speech_spans(&audio.samples, audio.sample_rate, &VadConfig::default());
    assert_eq!(spans.len(), 1, "{spans:?}");
    assert!((spans[0].start_s - 0.4).abs() < 0.15, "{:?}", spans[0]);
    assert!((spans[0].end_s - 2.1).abs() < 0.15, "{:?}", spans[0]);
}

#[test]
fn transcribes_into_timed_segment_documents() {
    let calls = fake("fake-test-segments");
    let documents = load_documents_with(
        &wav_fixture(),
        "wav",
        Some("episode.wav"),
        None,
        Some("fake-test-segments"),
    )
    .unwrap();

    assert_eq!(documents.len(), 3);
    assert_eq!(calls.load(Ordering::Relaxed), 1);

    let first = &documents[0];
    assert_eq!(first.content, "Welcome to the show.");
    assert_eq!(
        first.metadata.get("format").unwrap(),
        &serde_json::json!("audio")
    );
    assert_eq!(
        first.metadata.get("transcriber").unwrap(),
        &serde_json::json!("fake-test-segments")
    );
    assert_eq!(
        first.metadata.get("audio_format").unwrap(),
        &serde_json::json!("wav")
    );
    assert_eq!(
        first.metadata.get("total_segments").unwrap(),
        &serde_json::json!(3)
    );
    assert_eq!(
        first.metadata.get("segment_index").unwrap(),
        &serde_json::json!(0)
    );
    assert_eq!(
        first.metadata.get("speaker").unwrap(),
        &serde_json::json!("Host")
    );
    let start = first
        .metadata
        .get("start_time_s")
        .unwrap()
        .as_f64()
        .unwrap();
    let end = first.metadata.get("end_time_s").unwrap().as_f64().unwrap();
    assert!((start - 0.5).abs() < 1e-9 && (end - 1.2).abs() < 1e-9);

    // Speech measurement rides along, so callers can spot mostly-silent audio.
    let ratio = first
        .metadata
        .get("speech_ratio")
        .unwrap()
        .as_f64()
        .unwrap();
    assert!(ratio > 0.4 && ratio < 0.85, "speech ratio {ratio}");
    assert_eq!(
        documents[2].metadata.get("speaker").unwrap(),
        &serde_json::json!("Guest")
    );
}

#[test]
fn transcript_segments_merge_into_time_windows() {
    let _ = fake("fake-test-windows");
    let documents = load_documents_with(
        &wav_fixture(),
        "wav",
        Some("episode.wav"),
        None,
        Some("fake-test-windows"),
    )
    .unwrap();

    // One second per window: the first two segments share a window, the third
    // cannot fit without splitting it.
    let chunker = SubtitleChunker::new().with_max_seconds(1.6);
    let windows = chunker.chunk_documents(&documents).unwrap();

    assert_eq!(
        windows.len(),
        2,
        "{}",
        serde_json::to_string(&windows).unwrap()
    );
    let first = &windows[0];
    assert!(
        first.content.contains("Welcome to the show.") && first.content.contains("chunking."),
        "{}",
        first.content
    );
    assert_eq!(
        first.metadata.get("cue_count").unwrap(),
        &serde_json::json!(2)
    );
    // Window provenance survives: source, transcriber, spans.
    assert_eq!(
        first.metadata.get("source").unwrap(),
        &serde_json::json!("episode.wav")
    );
    assert_eq!(
        first.metadata.get("transcriber").unwrap(),
        &serde_json::json!("fake-test-windows")
    );
    assert!(
        first
            .metadata
            .get("start_time_s")
            .unwrap()
            .as_f64()
            .unwrap()
            >= 0.4
    );
    assert!(first.metadata.get("end_time_s").unwrap().as_f64().unwrap() <= 2.0);
    assert!(
        windows[1]
            .metadata
            .get("start_time_s")
            .unwrap()
            .as_f64()
            .unwrap()
            >= 2.0
    );
}

#[test]
fn audio_formats_join_the_dispatcher() {
    let supported = chunkr::loader::auto::supported_extensions();
    for extension in ["wav", "mp3", "flac", "ogg", "m4a"] {
        assert!(supported.contains(&extension.to_string()), "{extension}");
    }
    assert!(!supported.contains(&"opus".to_string()), "no Opus decoder");
    assert!(chunkr::loader::auto::is_structured("wav"));
    assert!(chunkr::loader::auto::is_audio("mp3"));
    assert!(!chunkr::loader::auto::is_audio("txt"));
}

#[test]
fn silence_only_audio_fails_before_reaching_a_model() {
    let _ = fake("fake-test-silence");
    let silence = encode_wav(&vec![0.0f32; 32_000], 16_000);
    let error = load_documents_with(&silence, "wav", Some("quiet.wav"), None, None).unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("no speech detected") && message.contains("quiet.wav"),
        "{message}"
    );
}

#[test]
fn a_transcriber_that_finds_nothing_is_an_error() {
    register_transcriber(
        "fake-test-empty",
        Box::new(FakeTranscriber {
            name: "fake-test-empty",
            segments: Vec::new(),
            calls: Arc::new(AtomicUsize::new(0)),
        }),
    );
    let error = load_documents_with(
        &wav_fixture(),
        "wav",
        Some("episode.wav"),
        None,
        Some("fake-test-empty"),
    )
    .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("returned no segments"), "{message}");
}

#[test]
fn unknown_or_corrupt_audio_names_the_source() {
    let error = load_documents_with(b"not audio at all", "mp3", Some("broken.mp3"), None, None)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("broken.mp3") && error.contains("audio"),
        "{error}"
    );

    let error = load_documents_with(
        &wav_fixture(),
        "wav",
        Some("episode.wav"),
        None,
        Some("no-such-transcriber"),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("no-such-transcriber"), "{error}");
    assert!(error.contains("available:"), "{error}");
}

#[test]
fn registering_replaces_and_unregistering_removes() {
    fake("fake-test-lifecycle");
    assert!(registered_transcribers().contains(&"fake-test-lifecycle".to_string()));

    // Re-registering the same name replaces instead of duplicating.
    fake("fake-test-lifecycle");
    let count = registered_transcribers()
        .iter()
        .filter(|name| name.as_str() == "fake-test-lifecycle")
        .count();
    assert_eq!(count, 1);

    assert!(unregister_transcriber("fake-test-lifecycle"));
    assert!(!unregister_transcriber("fake-test-lifecycle"));
    assert!(!registered_transcribers().contains(&"fake-test-lifecycle".to_string()));
}
