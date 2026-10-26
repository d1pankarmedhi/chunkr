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

/// Tests that touch `CHUNKR_*` variables share the process environment, so they
/// take this lock instead of racing each other.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    // `decode` is the pure path — no environment, no ffmpeg bridge — so this
    // asserts the built-in error rather than whatever a parallel test left behind.
    let error = chunkr::audio::decode(b"not audio at all", "mp3").expect_err("garbage");
    assert!(error.contains("audio"), "{error}");

    // Naming the source happens for a valid but silent file, which fails before a
    // transcriber is ever chosen.
    let silent = encode_wav(&vec![0.0; 16_000], 16_000);
    let error = load_documents_with(&silent, "wav", Some("episode.wav"), None, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("episode.wav"), "{error}");
    assert!(error.contains("no speech"), "{error}");

    // Something must be registered for the "unknown name" branch to be reached.
    fake("fake-for-unknown-name");
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

// ── Resampling ──────────────────────────────────────────────────────────────

/// Count zero crossings, which is twice the frequency for a clean tone.
fn zero_crossings(samples: &[f32]) -> usize {
    samples
        .windows(2)
        .filter(|pair| (pair[0] < 0.0) != (pair[1] < 0.0))
        .count()
}

#[test]
fn resampling_keeps_pitch_and_duration() {
    // A 220 Hz tone recorded at 44.1 kHz is what a real recording looks like to
    // a model that insists on 16 kHz; it must still be 220 Hz afterwards.
    let mut samples = Vec::new();
    tone(&mut samples, 2.0, 44_100);
    let audio = chunkr::audio::DecodedAudio {
        samples,
        sample_rate: 44_100,
        channels: 1,
        duration_s: 2.0,
    };

    let resampled = chunkr::audio::resample(&audio, 16_000);
    assert_eq!(resampled.sample_rate, 16_000);
    assert!((resampled.duration_s - 2.0).abs() < 0.01);

    let frequency = zero_crossings(&resampled.samples) as f64 / 2.0 / resampled.duration_s;
    assert!(
        (frequency - 220.0).abs() < 5.0,
        "220 Hz became {frequency:.1} Hz"
    );

    // Upsampling back keeps it, so the filter is not a one-way loss.
    let restored = chunkr::audio::resample(&resampled, 44_100);
    assert!((restored.duration_s - 2.0).abs() < 0.01);
}

#[test]
fn resampling_a_speech_rate_file_is_a_no_op() {
    let audio = chunkr::audio::DecodedAudio {
        samples: vec![0.1, -0.1, 0.1, -0.1],
        sample_rate: 16_000,
        channels: 1,
        duration_s: 4.0 / 16_000.0,
    };
    let same = chunkr::audio::resample(&audio, 16_000);
    assert_eq!(same.samples, audio.samples);
    assert_eq!(same.sample_rate, 16_000);
}

// ── whisper.cpp subprocess bridge ───────────────────────────────────────────

#[cfg(unix)]
mod whisper_cpp {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use chunkr::audio::{
        encode_wav, load_documents_with, register_transcriber, unregister_transcriber,
    };

    /// Write a shell script that stands in for `whisper-cli`.
    fn script(body: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "chunkr-fake-whisper-{}-{}.sh",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    fn speech_wav(rate: u32, seconds: f64) -> Vec<u8> {
        let mut samples = Vec::new();
        for index in 0..(seconds * rate as f64) as usize {
            let phase = 2.0 * std::f32::consts::PI * 220.0 * index as f32 / rate as f32;
            samples.push(phase.sin() * 0.4);
        }
        encode_wav(&samples, rate)
    }

    #[test]
    fn runs_the_binary_on_a_16khz_copy_and_maps_the_segments() {
        // The fake records the WAV it was handed, then writes whisper.cpp's `-oj`
        // shape, including the markers the real model emits.
        let captured = std::env::temp_dir().join(format!(
            "chunkr-captured-{}-{}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let out_prefix = captured.with_extension("prefix");
        let json = r#"{"transcription":[
                {"offsets":{"from":0,"to":2500},"text":" He hoped there would be stew."},
                {"offsets":{"from":2500,"to":3000},"text":" [BLANK_AUDIO]"},
                {"offsets":{"from":3000,"to":4200},"text":" For dinner. "}
            ]}"#;
        let script = script(&format!(
            "input=\"\"; out=\"\"\n\
             while [ $# -gt 0 ]; do\n\
               case \"$1\" in -f) input=\"$2\"; shift 2;; -of) out=\"$2\"; shift 2;; *) shift;; esac\n\
             done\n\
             cp \"$input\" '{captured}'\n\
             printf '%s' '{json}' > \"$out.json\"\n",
            captured = captured.display(),
            json = json.replace('\n', " ")
        ));
        let _ = out_prefix;

        register_transcriber(
            "test-whisper-cpp",
            Box::new(chunkr::audio::CommandTranscriber::whisper_cpp(
                &script,
                "/models/ggml-tiny.en.bin",
            )),
        );
        let result = load_documents_with(
            &speech_wav(44_100, 5.0),
            "wav",
            Some("clip.wav"),
            None,
            Some("test-whisper-cpp"),
        );
        unregister_transcriber("test-whisper-cpp");
        let _ = std::fs::remove_file(&script);

        let documents = result.expect("transcribed");
        assert_eq!(documents.len(), 2, "marker dropped: {documents:#?}");
        assert_eq!(documents[0].content, "He hoped there would be stew.");
        assert_eq!(documents[0].metadata["start_time_s"], 0.0);
        assert_eq!(documents[0].metadata["end_time_s"], 2.5);
        assert_eq!(documents[1].content, "For dinner.");
        assert_eq!(documents[1].metadata["end_time_s"], 4.2);
        assert_eq!(
            documents[0].metadata["transcriber"],
            "whisper.cpp/ggml-tiny.en"
        );

        // The binary must receive 16 kHz mono, whatever the input was.
        let captured_bytes = std::fs::read(&captured).expect("captured wav");
        let header = &captured_bytes[..44];
        let channels = u16::from_le_bytes([header[22], header[23]]);
        let rate = u32::from_le_bytes([header[24], header[25], header[26], header[27]]);
        assert_eq!(channels, 1);
        assert_eq!(rate, 16_000);
        let _ = std::fs::remove_file(&captured);
    }

    #[test]
    fn a_failing_binary_reports_its_own_message() {
        let script = script(
            "echo 'error: failed to load model' >&2\n\
             exit 3",
        );
        register_transcriber(
            "test-whisper-fail",
            Box::new(chunkr::audio::CommandTranscriber::whisper_cpp(
                &script,
                "/models/tiny.bin",
            )),
        );
        let error = load_documents_with(
            &speech_wav(16_000, 3.0),
            "wav",
            Some("clip.wav"),
            None,
            Some("test-whisper-fail"),
        )
        .expect_err("exit 3 must fail");
        unregister_transcriber("test-whisper-fail");
        let _ = std::fs::remove_file(&script);

        let message = error.to_string();
        assert!(message.contains("failed to load model"), "{message}");
        assert!(message.contains("whisper.cpp"), "{message}");
    }

    #[test]
    fn a_missing_binary_says_how_to_fix_it() {
        register_transcriber(
            "test-whisper-missing",
            Box::new(chunkr::audio::CommandTranscriber::whisper_cpp(
                "/nonexistent/whisper-cli",
                "/models/tiny.bin",
            )),
        );
        let error = load_documents_with(
            &speech_wav(16_000, 3.0),
            "wav",
            Some("clip.wav"),
            None,
            Some("test-whisper-missing"),
        )
        .expect_err("missing binary must fail");
        unregister_transcriber("test-whisper-missing");

        let message = error.to_string();
        assert!(message.contains("CHUNKR_WHISPER_BIN"), "{message}");
        assert!(message.contains("chunkr-audio"), "{message}");
    }

    #[test]
    fn registering_from_the_environment_only_happens_once() {
        let _guard = super::ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // The dispatcher calls this on every load; it must not pile up duplicates.
        std::env::set_var("CHUNKR_WHISPER_BIN", "/usr/bin/true");
        std::env::set_var("CHUNKR_WHISPER_MODEL", "/models/tiny.bin");
        std::env::remove_var("CHUNKR_FFMPEG");
        chunkr::audio::register_from_env();
        chunkr::audio::register_from_env();
        let registered = chunkr::audio::registered_transcribers();
        std::env::remove_var("CHUNKR_WHISPER_BIN");
        std::env::remove_var("CHUNKR_WHISPER_MODEL");
        unregister_transcriber("whisper-cpp");

        assert_eq!(
            registered
                .iter()
                .filter(|name| *name == "whisper-cpp")
                .count(),
            1
        );
    }
}

// ── ffmpeg fallback for codecs the core cannot decode ───────────────────────

#[cfg(unix)]
mod ffmpeg_fallback {
    use std::os::unix::fs::PermissionsExt;

    use chunkr::audio::decode_with_fallback;

    /// A stub `ffmpeg` that ignores its input and writes a WAV to stdout, which is
    /// exactly the contract the fallback relies on (`-f wav pipe:1`).
    fn stub_ffmpeg() -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("chunkr-fake-ffmpeg-{}.sh", std::process::id()));
        let samples = {
            let mut samples = Vec::new();
            for index in 0..8_000 {
                let phase = 2.0 * std::f32::consts::PI * 220.0 * index as f32 / 16_000.0;
                samples.push(phase.sin() * 0.3);
            }
            chunkr::audio::encode_wav(&samples, 16_000)
        };
        let wav_path = path.with_extension("wav");
        std::fs::write(&wav_path, &samples).expect("write wav");
        std::fs::write(&path, format!("#!/bin/sh\ncat '{}'\n", wav_path.display()))
            .expect("write stub");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    #[test]
    fn an_opus_like_file_decodes_through_the_configured_binary() {
        // Not real Opus: the point is that a decode failure routes through the
        // configured binary instead of returning the built-in error.
        let _guard = super::ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let stub = stub_ffmpeg();
        std::env::set_var("CHUNKR_FFMPEG", &stub);

        let audio = decode_with_fallback(b"OpusHead not really opus", "ogg")
            .expect("fallback should decode");
        std::env::remove_var("CHUNKR_FFMPEG");
        let _ = std::fs::remove_file(&stub);
        let _ = std::fs::remove_file(stub.with_extension("wav"));

        assert_eq!(audio.sample_rate, 16_000);
        assert!((audio.duration_s - 0.5).abs() < 0.01);
    }

    #[test]
    fn a_broken_binary_reports_both_errors() {
        let _guard = super::ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let path =
            std::env::temp_dir().join(format!("chunkr-bad-ffmpeg-{}.sh", std::process::id()));
        std::fs::write(&path, "#!/bin/sh\necho 'no such codec' >&2\nexit 1\n").expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        std::env::set_var("CHUNKR_FFMPEG", &path);

        let error = decode_with_fallback(b"OpusHead broken", "ogg").expect_err("must fail");
        std::env::remove_var("CHUNKR_FFMPEG");
        let _ = std::fs::remove_file(&path);

        assert!(error.contains("ffmpeg fallback failed"), "{error}");
        assert!(error.contains("no such codec"), "{error}");
    }
}

// ── Speech models: manifest, cache and resolution ───────────────────────────

mod speech_models {
    use chunkr::audio::models;

    #[test]
    fn the_manifest_pins_every_model_it_offers() {
        assert!(models::MODELS.len() >= 4);
        let mut names = Vec::new();
        for spec in models::MODELS {
            assert!(!spec.name.is_empty());
            assert_eq!(spec.sha256.len(), 64, "{} has no full digest", spec.name);
            assert!(spec.sha256.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(spec.bytes > 1_000_000, "{} looks like a stub", spec.name);
            assert!(
                spec.url.starts_with("https://"),
                "{} must be TLS",
                spec.name
            );
            assert!(!names.contains(&spec.name), "duplicate {}", spec.name);
            names.push(spec.name);
        }
        // Both language families are reachable by name.
        assert!(models::spec("base.en").is_some());
        assert!(models::spec("base").is_some());
        assert!(models::spec("nope").is_none());
    }

    #[test]
    fn the_cache_directory_follows_the_environment() {
        let _guard = super::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("CHUNKR_MODEL_DIR", "/tmp/chunkr-models-test");
        assert_eq!(
            models::cache_dir(),
            std::path::PathBuf::from("/tmp/chunkr-models-test")
        );
        std::env::remove_var("CHUNKR_MODEL_DIR");
        assert!(
            models::cache_dir().ends_with("models"),
            "default cache dir should end in models: {:?}",
            models::cache_dir()
        );
    }

    #[test]
    fn resolving_a_missing_model_says_how_to_get_it() {
        let _guard = super::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("CHUNKR_MODEL_DIR", "/tmp/chunkr-empty-cache");
        let error = models::resolve("tiny.en").expect_err("nothing cached");
        std::env::remove_var("CHUNKR_MODEL_DIR");

        assert!(error.contains("chunkr models pull tiny.en"), "{error}");

        let unknown = models::resolve("gigantic").expect_err("unknown name");
        assert!(unknown.contains("known:"), "{unknown}");
        assert!(unknown.contains("base.en"), "{unknown}");
    }

    #[test]
    fn a_path_is_taken_as_given() {
        // A user with their own quantisation must not be second-guessed.
        let dir = std::env::temp_dir().join(format!("chunkr-model-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("custom-ggml.bin");
        std::fs::write(&file, b"not really a model").expect("write");

        assert_eq!(models::resolve(file.to_str().unwrap()).unwrap(), file);
        assert!(models::resolve(dir.join("missing.bin").to_str().unwrap()).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_truncated_cache_entry_is_not_trusted() {
        let _guard = super::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("chunkr-partial-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("ggml-tiny.en.bin"), b"partial").expect("write");
        std::env::set_var("CHUNKR_MODEL_DIR", dir.display().to_string());

        let spec = models::spec("tiny.en").expect("known");
        assert!(
            models::cached(spec).is_none(),
            "a half-downloaded file must not count as cached"
        );

        std::env::remove_var("CHUNKR_MODEL_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_binary_is_found_through_the_environment() {
        let _guard = super::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("CHUNKR_WHISPER_BIN", "/bin/sh");
        assert_eq!(
            models::find_binary(),
            Some(std::path::PathBuf::from("/bin/sh"))
        );
        std::env::set_var("CHUNKR_WHISPER_BIN", "/nonexistent/whisper");
        assert!(
            models::find_binary().is_none()
                || models::find_binary() != Some("/nonexistent/whisper".into()),
            "a bogus path must not be returned"
        );
        std::env::remove_var("CHUNKR_WHISPER_BIN");
    }
}

// ── Speech detection must never deny a file the model could read ────────────

mod vad_guard {
    use chunkr::audio::{decode, encode_wav, load_documents_with, speech_spans, VadConfig};

    /// Naive broadband noise: crossing zero almost every sample, unlike speech.
    fn noise(seconds: f64, rate: u32, amplitude: f32) -> Vec<f32> {
        let mut state: u32 = 0x1234_5678;
        (0..(seconds * rate as f64) as usize)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                // Bipolar, like real noise: the mean has to come out.
                ((state >> 16) as f32 / 32768.0 - 1.0) * amplitude
            })
            .collect()
    }

    #[test]
    fn white_noise_is_not_counted_as_speech() {
        let rate = 16_000;
        let samples = noise(3.0, rate, 0.3);
        let spans = speech_spans(&samples, rate, &VadConfig::default());
        let speech: f64 = spans.iter().map(|span| span.end_s - span.start_s).sum();
        assert!(
            speech / 3.0 < 0.2,
            "broadband noise counted as {:.0}% speech",
            speech / 3.0 * 100.0
        );
    }

    #[test]
    fn a_tone_with_gaps_is_still_found() {
        // Voiced-ish: a tone is low-ZCR, so the noise gate must leave it alone.
        let rate = 16_000;
        let mut samples = vec![0.0f32; rate as usize];
        for (index, sample) in samples.iter_mut().enumerate().take(rate as usize) {
            if (rate / 2) < index as u32 {
                let phase = 2.0 * std::f32::consts::PI * 220.0 * index as f32 / rate as f32;
                *sample = phase.sin() * 0.3;
            }
        }
        let spans = speech_spans(&samples, rate, &VadConfig::default());
        assert_eq!(spans.len(), 1, "{spans:?}");
        assert!(spans[0].start_s > 0.3 && spans[0].end_s < 1.1, "{spans:?}");
    }

    #[test]
    fn noisy_speech_reaches_the_transcriber_instead_of_being_refused() {
        // The measured failure of an energy VAD: speech buried in noise at 0 dB SNR
        // yields no spans. That must not deny the file — the model decides.
        let rate = 16_000;
        let mut samples = vec![0.0f32; (3.0 * rate as f64) as usize];
        for (index, sample) in samples.iter_mut().enumerate() {
            let phase = 2.0 * std::f32::consts::PI * 180.0 * index as f32 / rate as f32;
            *sample = phase.sin() * 0.05;
        }
        // Hiss well above the tone: this is what 0 dB SNR looks like to the
        // detector, and it is why the guard cannot be the VAD's verdict.
        let loud_noise = noise(3.0, rate, 0.25);
        for (sample, hiss) in samples.iter_mut().zip(loud_noise) {
            *sample += hiss;
        }
        let wav = encode_wav(&samples, rate);

        // Force the VAD to find nothing by checking the premise first.
        let audio = decode(&wav, "wav").expect("decode");
        let spans = speech_spans(&audio.samples, rate, &VadConfig::default());
        assert!(
            spans.is_empty(),
            "premise changed: the detector now finds {spans:?}, so this no longer \
             exercises the guard"
        );

        crate::fake("fake-for-noisy-audio");
        let documents = load_documents_with(
            &wav,
            "wav",
            Some("noisy.wav"),
            None,
            Some("fake-for-noisy-audio"),
        )
        .expect("a noisy file must reach the transcriber");
        assert!(!documents.is_empty());
        assert_eq!(
            documents[0].metadata["speech_detected"],
            serde_json::json!(false)
        );
        assert_eq!(
            documents[0].metadata["speech_ratio"],
            serde_json::json!(0.0)
        );
    }

    #[test]
    fn silent_audio_is_still_refused_before_a_model_is_called() {
        let wav = encode_wav(&vec![0.0; 16_000], 16_000);
        let error = load_documents_with(&wav, "wav", Some("empty.wav"), None, None)
            .expect_err("silence must fail");
        let message = error.to_string();
        assert!(message.contains("empty.wav"), "{message}");
        assert!(message.contains("no speech"), "{message}");
    }
}

// ── Word timings and quiet-seam windows ────────────────────────────────────

mod words_and_windows {
    use super::ENV_LOCK;
    use chunkr::audio::{
        encode_wav, load_documents_with, register_transcriber, unregister_transcriber,
        window_bounds, SpeechSpan, Transcriber, TranscriptSegment, VadConfig, WordTiming,
    };
    use chunkr::error::ChunkrError;
    use serde_json::json;

    /// A tone with silent gaps, so seam placement has something to find.
    fn speech(rate: u32, seconds: f64) -> Vec<f32> {
        let mut samples = vec![0.0f32; (seconds * rate as f64) as usize];
        for (index, sample) in samples.iter_mut().enumerate() {
            let t = index as f32 / rate as f32;
            let in_gap =
                (2.0..2.3).contains(&t) || (4.0..4.3).contains(&t) || (6.0..6.3).contains(&t);
            if !in_gap {
                *sample = (2.0 * std::f32::consts::PI * 220.0 * t).sin() * 0.4;
            }
        }
        samples
    }

    #[test]
    fn window_bounds_never_drop_audio_and_respect_the_limit() {
        let samples = speech(16_000, 8.0);
        let bounds = window_bounds(&samples, 16_000, 3.0, 0.5);
        assert!(bounds.len() >= 3, "{bounds:?}");
        assert_eq!(bounds[0].0, 0);
        assert_eq!(bounds.last().unwrap().1, samples.len());
        for pair in bounds.windows(2) {
            assert_eq!(pair[0].1, pair[1].0);
        }
        for (start, end) in &bounds {
            assert!((end - start) as f64 / 16_000.0 <= 3.0 + 0.001);
        }
    }

    /// Reports how many seconds it was handed and how many calls it saw.
    struct WindowProbe {
        seen: std::sync::Arc<std::sync::Mutex<Vec<f64>>>,
    }

    impl Transcriber for WindowProbe {
        fn name(&self) -> String {
            "window-probe".to_string()
        }

        fn transcribe(
            &self,
            source: &chunkr::audio::AudioSource<'_>,
        ) -> Result<Vec<TranscriptSegment>, ChunkrError> {
            self.seen.lock().unwrap().push(source.audio.duration_s);
            // One segment per call, at the start of the piece it was given.
            Ok(vec![TranscriptSegment::new(0.0, 0.5, "chunk").with_words(
                vec![
                    WordTiming::new("chunk", 0.0, 0.25),
                    WordTiming::new("two", 0.25, 0.5),
                ],
            )])
        }
    }

    #[test]
    fn chunking_windows_cuts_the_payload_and_offsets_the_transcript() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let rate = 16_000;
        let samples = speech(rate, 8.0);
        let wav = encode_wav(&samples, rate);

        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        register_transcriber("window-probe", Box::new(WindowProbe { seen: seen.clone() }));
        std::env::set_var("CHUNKR_AUDIO_WINDOW_S", "3");

        let documents =
            load_documents_with(&wav, "wav", Some("talk.wav"), None, Some("window-probe"))
                .expect("windowed transcription");

        let durations = seen.lock().unwrap().clone();
        assert!(
            durations.len() >= 3,
            "expected several calls, got {durations:?}"
        );
        for duration in &durations {
            assert!(
                *duration <= 3.0 + 0.001,
                "call of {duration:.2}s exceeds the window"
            );
        }
        // Windows tile the file, so the offsets must be increasing and inside it.
        let starts: Vec<f64> = documents
            .iter()
            .map(|document| document.metadata["start_time_s"].as_f64().unwrap())
            .collect();
        assert!(
            starts.windows(2).all(|pair| pair[0] <= pair[1]),
            "{starts:?}"
        );
        assert_eq!(
            documents[0].metadata["audio_windows"],
            json!(durations.len())
        );
        assert_eq!(documents[0].metadata["word_count"], json!(2));
        assert!(starts.last().unwrap() > &3.0, "{starts:?}");

        unregister_transcriber("window-probe");
        std::env::remove_var("CHUNKR_AUDIO_WINDOW_S");
    }

    #[test]
    fn without_the_env_var_the_payload_is_sent_once() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let rate = 16_000;
        let wav = encode_wav(&speech(rate, 8.0), rate);
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        register_transcriber("window-probe", Box::new(WindowProbe { seen: seen.clone() }));
        std::env::remove_var("CHUNKR_AUDIO_WINDOW_S");

        let documents = load_documents_with(&wav, "wav", None, None, Some("window-probe")).unwrap();
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].metadata["word_count"], json!(2));
        assert!(!documents[0].metadata.contains_key("audio_windows"));

        unregister_transcriber("window-probe");
    }

    #[test]
    fn words_land_in_flat_metadata() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        struct Wordy;
        impl Transcriber for Wordy {
            fn name(&self) -> String {
                "wordy".to_string()
            }
            fn transcribe(
                &self,
                _source: &chunkr::audio::AudioSource<'_>,
            ) -> Result<Vec<TranscriptSegment>, ChunkrError> {
                Ok(vec![TranscriptSegment::new(0.0, 1.5, "hello there")
                    .with_words(vec![
                        WordTiming::new("hello", 0.0, 0.6),
                        WordTiming::new("there", 0.6, 1.5),
                    ])])
            }
        }
        register_transcriber("wordy", Box::new(Wordy));
        let wav = encode_wav(&speech(16_000, 2.0), 16_000);
        let documents = load_documents_with(&wav, "wav", None, None, Some("wordy")).unwrap();
        let metadata = &documents[0].metadata;
        assert_eq!(metadata["word_count"], json!(2));
        assert_eq!(metadata["first_word_s"], json!(0.0));
        assert_eq!(metadata["last_word_s"], json!(1.5));
        // Words are timings, not payload: nothing nested leaks into metadata.
        assert!(
            !metadata.keys().any(|key| key.contains("words\"")),
            "{metadata:?}"
        );
        unregister_transcriber("wordy");
    }

    #[test]
    fn vad_spans_stay_the_seam_a_model_based_detector_would_plug_into() {
        // The windowing code takes the same shape a neural VAD would return.
        let spans =
            chunkr::audio::speech_spans(&speech(16_000, 8.0), 16_000, &VadConfig::default());
        assert!(!spans.is_empty());
        assert!(spans
            .iter()
            .all(|span: &SpeechSpan| span.end_s > span.start_s));
    }
}

mod whisper_cpp_words {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use chunkr::audio::{
        encode_wav, load_documents_with, register_transcriber, unregister_transcriber,
        CommandTranscriber,
    };
    use serde_json::json;

    fn script(body: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("chunkr-words-whisper-{}.sh", std::process::id()));
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    #[test]
    fn token_offsets_become_words() {
        // whisper.cpp with `-ojf`: tokens carry offsets; sub-word pieces and
        // trailing punctuation must not become words of their own, and special
        // tokens must not become text.
        let json = json!({
            "transcription": [{
                "offsets": {"from": 0, "to": 2600},
                "text": " I have a dream.",
                "tokens": [
                    {"text": "[_BEG_]", "offsets": {"from": 0, "to": 0}},
                    {"text": " I", "offsets": {"from": 0, "to": 180}},
                    {"text": " have", "offsets": {"from": 180, "to": 900}},
                    {"text": " a", "offsets": {"from": 1060, "to": 1200}},
                    {"text": " dre", "offsets": {"from": 1200, "to": 1700}},
                    {"text": "am", "offsets": {"from": 1700, "to": 2000}},
                    {"text": ".", "offsets": {"from": 2000, "to": 2100}},
                    {"text": " There", "offsets": {"from": 2100, "to": 2600}}
                ]
            }]
        })
        .to_string();
        let script = script(&format!(
            "while [ $# -gt 0 ]; do\n\
               case \"$1\" in -of) out=\"$2\"; shift 2;; *) shift;; esac\n\
             done\n\
             printf '%s' '{json}' > \"$out.json\"\n"
        ));
        register_transcriber(
            "words-whisper",
            Box::new(CommandTranscriber::whisper_cpp(&script, "/models/tiny.bin")),
        );
        let samples: Vec<f32> = (0..16_000 * 3)
            .map(|index| (2.0 * std::f32::consts::PI * 220.0 * index as f32 / 16_000.0).sin() * 0.4)
            .collect();
        let wav = encode_wav(&samples, 16_000);
        let documents =
            load_documents_with(&wav, "wav", Some("clip.wav"), None, Some("words-whisper"))
                .expect("transcription with words");
        let metadata = &documents[0].metadata;
        assert_eq!(metadata["word_count"], json!(5), "{metadata:?}");
        assert_eq!(metadata["first_word_s"], json!(0.0));
        assert_eq!(metadata["last_word_s"], json!(2.6));
        unregister_transcriber("words-whisper");
    }
}
