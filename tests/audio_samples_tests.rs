//! End-to-end checks against real recordings.
//!
//! The fixtures are not in the repository, so these tests are `#[ignore]`d and
//! read whatever directory `CHUNKR_AUDIO_SAMPLES` points at. They exist because
//! synthetic tones prove the plumbing and prove nothing about real containers:
//! every codec here was validated on audio downloaded from
//! <https://huggingface.co/datasets/Narsil/asr_dummy> (LibriSpeech clips, an mp3,
//! an ogg, plus an AAC `.m4a` produced with `afconvert`).
//!
//! ```bash
//! mkdir -p /tmp/hf_audio && cd /tmp/hf_audio
//! for f in 1.flac 2.flac 3.flac i-know-kung-fu.mp3 hindi.ogg; do
//!   curl -LO "https://huggingface.co/datasets/Narsil/asr_dummy/resolve/main/$f"
//! done
//! afconvert -f m4af -d aac 1.flac sample.m4a
//!
//! CHUNKR_AUDIO_SAMPLES=/tmp/hf_audio cargo test --features audio \
//!   --test audio_samples_tests -- --ignored --nocapture
//! ```
//!
//! Set `CHUNKR_WHISPER_BIN` and `CHUNKR_WHISPER_MODEL` as well to include the
//! transcription cases (a ggml model from `ggerganov/whisper.cpp`).

#![cfg(all(feature = "audio", not(target_arch = "wasm32")))]

use std::path::{Path, PathBuf};

use chunkr::audio::{decode, decode_with_fallback, resample, speech_spans, VadConfig};

fn samples_dir() -> PathBuf {
    match std::env::var("CHUNKR_AUDIO_SAMPLES") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => panic!("set CHUNKR_AUDIO_SAMPLES to a directory of audio files"),
    }
}

/// Decode, treating Opus without an `ffmpeg` bridge as a skip rather than a
/// failure: that combination is a documented gap, and pretending otherwise would
/// hide the formats that do work.
fn decode_or_skip(
    bytes: &[u8],
    extension: &str,
    path: &Path,
) -> Option<chunkr::audio::DecodedAudio> {
    match decode_with_fallback(bytes, extension) {
        Ok(audio) => Some(audio),
        Err(error) if error.contains("CHUNKR_FFMPEG") => {
            println!("skip {}: {error}", path.display());
            None
        }
        Err(error) => panic!("{}: {error}", path.display()),
    }
}

fn files() -> Vec<(PathBuf, String)> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(samples_dir()).expect("samples dir") {
        let path = entry.expect("entry").path();
        let extension = path
            .extension()
            .map(|value| value.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if chunkr::loader::auto::is_audio(&extension) {
            found.push((path, extension));
        }
    }
    found.sort();
    assert!(!found.is_empty(), "no audio files in {:?}", samples_dir());
    found
}

#[test]
#[ignore = "needs CHUNKR_AUDIO_SAMPLES"]
fn decodes_every_real_container_and_finds_speech() {
    let mut seen = Vec::new();
    for (path, extension) in files() {
        let bytes = std::fs::read(&path).expect("read sample");
        let Some(audio) = decode_or_skip(&bytes, &extension, &path) else {
            continue;
        };

        let spans = speech_spans(&audio.samples, audio.sample_rate, &VadConfig::default());
        let speech_s: f64 = spans.iter().map(|span| span.end_s - span.start_s).sum();
        println!(
            "{:<22} {:>6} Hz  {:>6.2} s  peak {:.2}  speech {:>5.2} s  {:?}",
            path.file_name().unwrap().to_string_lossy(),
            audio.sample_rate,
            audio.duration_s,
            audio.peak(),
            speech_s,
            spans
                .iter()
                .map(|span| format!("{:.2}-{:.2}", span.start_s, span.end_s))
                .collect::<Vec<_>>()
        );

        assert!(
            audio.duration_s > 1.0,
            "{}: implausible duration",
            path.display()
        );
        assert!(audio.peak() > 0.01, "{}: silent", path.display());
        assert!(
            !spans.is_empty(),
            "{}: no speech found in real speech",
            path.display()
        );
        assert!(
            speech_s < audio.duration_s + 0.05,
            "{}: speech longer than audio",
            path.display()
        );
        seen.push(extension);
    }

    // Whatever was staged, more than one container was exercised.
    assert!(seen.len() > 1, "stage at least two formats: {seen:?}");
}

#[test]
#[ignore = "needs CHUNKR_AUDIO_SAMPLES"]
fn resampling_preserves_duration_and_speech() {
    for (path, extension) in files() {
        let bytes = std::fs::read(&path).expect("read sample");
        let Some(audio) = decode_or_skip(&bytes, &extension, &path) else {
            continue;
        };
        let resampled = resample(&audio, 16_000);

        let spans = speech_spans(&resampled.samples, 16_000, &VadConfig::default());
        let speech_s: f64 = spans.iter().map(|span| span.end_s - span.start_s).sum();
        println!(
            "{:<22} {} Hz {:.2} s -> {} Hz {:.2} s, speech {:.2} s",
            path.file_name().unwrap().to_string_lossy(),
            audio.sample_rate,
            audio.duration_s,
            resampled.sample_rate,
            resampled.duration_s,
            speech_s
        );

        assert_eq!(resampled.sample_rate, 16_000);
        assert!(
            (resampled.duration_s - audio.duration_s).abs() < 0.05,
            "{}: {} s became {} s after resampling",
            path.display(),
            audio.duration_s,
            resampled.duration_s
        );
        assert!(
            !spans.is_empty(),
            "{}: resampling lost the speech",
            path.display()
        );
    }
}

#[test]
#[ignore = "needs CHUNKR_AUDIO_SAMPLES and CHUNKR_WHISPER_BIN"]
fn transcribes_real_speech_with_a_local_whisper() {
    if std::env::var("CHUNKR_WHISPER_BIN").is_err()
        || std::env::var("CHUNKR_WHISPER_MODEL").is_err()
    {
        panic!("set CHUNKR_WHISPER_BIN and CHUNKR_WHISPER_MODEL");
    }

    for (path, extension) in files() {
        let bytes = std::fs::read(&path).expect("read sample");
        let documents = match chunkr::audio::load_documents(
            &bytes,
            &extension,
            Some(&path.display().to_string()),
            Some(&path),
        ) {
            Ok(documents) => documents,
            Err(error) if error.to_string().contains("CHUNKR_FFMPEG") => {
                println!("skip {}: {error}", path.display());
                continue;
            }
            Err(error) => panic!("{}: {error}", path.display()),
        };

        let transcript = documents
            .iter()
            .map(|document| document.content.trim())
            .collect::<Vec<_>>()
            .join(" ");
        println!(
            "{:<22} {} segment(s): {transcript}",
            path.file_name().unwrap().to_string_lossy(),
            documents.len()
        );

        assert!(!transcript.is_empty());
        for document in &documents {
            let start = document.metadata["start_time_s"].as_f64().expect("start");
            let end = document.metadata["end_time_s"].as_f64().expect("end");
            assert!(start < end, "{}: inverted span", path.display());
            assert!(
                end <= document.metadata["audio_duration_s"].as_f64().unwrap() + 1e-9,
                "{}: span past the end of the audio",
                path.display()
            );
        }
    }
}

#[test]
#[ignore = "needs CHUNKR_AUDIO_SAMPLES"]
fn opus_reports_a_usable_error() {
    // Opus is the one common codec symphonia cannot decode. The error has to name
    // it and point at the escape hatch, not say "unsupported codec".
    let path = samples_dir().join("hindi.ogg");
    if !Path::new(&path).exists() {
        panic!("stage hindi.ogg (Opus in Ogg) in CHUNKR_AUDIO_SAMPLES");
    }
    let bytes = std::fs::read(&path).expect("read sample");
    let error = decode(&bytes, "ogg").expect_err("opus has no built-in decoder");
    println!("{error}");
    assert!(error.contains("opus"), "error must name the codec: {error}");
}
