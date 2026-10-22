//! Audio through the public entry points: the format dispatcher and
//! `DirectoryLoader`. One test per binary, because the transcriber registry is
//! process-wide and the dispatcher picks the first registration.

#![cfg(all(feature = "audio", not(target_arch = "wasm32")))]

use std::sync::Arc;

use chunkr::audio::{encode_wav, register_transcriber, Transcriber, TranscriptSegment};
use chunkr::prelude::*;

struct Fixed;

impl Transcriber for Fixed {
    fn name(&self) -> String {
        "fixed-test".to_string()
    }

    fn transcribe(
        &self,
        source: &chunkr::audio::AudioSource<'_>,
    ) -> Result<Vec<TranscriptSegment>, ChunkrError> {
        // Prove the transcriber sees both the encoded bytes and the decoded audio.
        assert_eq!(source.format, "wav");
        assert!(!source.bytes.is_empty());
        assert!(source.audio.duration_s > 2.0);
        Ok(vec![
            TranscriptSegment::new(0.5, 1.2, "Welcome to the show.").with_speaker("Host"),
            TranscriptSegment::new(1.2, 2.0, "Today we talk about chunking.").with_speaker("Host"),
            TranscriptSegment::new(2.0, 2.4, "Great to be here.").with_speaker("Guest"),
        ])
    }
}

#[test]
fn audio_loads_and_chunks_by_time_through_the_dispatcher() {
    register_transcriber("fixed-test", Box::new(Fixed));

    let dir = std::env::temp_dir().join(format!("chunkr_audio_dispatch_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("episode.wav");
    let mut samples = vec![0.0f32; 8_000];
    for index in 0..24_000 {
        let phase = 2.0 * std::f32::consts::PI * 220.0 * index as f32 / 16_000.0;
        samples.push(phase.sin() * 0.3);
    }
    samples.extend(std::iter::repeat_n(0.0f32, 8_000));
    std::fs::write(&path, encode_wav(&samples, 16_000)).unwrap();

    // The dispatcher knows the extension and returns one document per segment.
    let documents = load_documents(&path).unwrap();
    assert_eq!(documents.len(), 3);
    assert_eq!(
        documents[0].metadata.get("format").unwrap(),
        &serde_json::json!("audio")
    );
    assert_eq!(
        documents[0].metadata.get("transcriber").unwrap(),
        &serde_json::json!("fixed-test")
    );

    // A directory load chunks audio by time window, keeping the spans.
    let chunks = DirectoryLoader::new()
        .with_chunk_size(10_000)
        .load_and_chunk(&dir)
        .unwrap();
    assert!(!chunks.is_empty());
    for chunk in &chunks {
        assert_eq!(
            chunk.metadata.get("format").unwrap(),
            &serde_json::json!("audio"),
            "audio chunk lost its format: {:?}",
            chunk.metadata
        );
        let start = chunk
            .metadata
            .get("start_time_s")
            .unwrap()
            .as_f64()
            .unwrap();
        let end = chunk.metadata.get("end_time_s").unwrap().as_f64().unwrap();
        assert!(end > start, "non-monotonic window: {start}..{end}");
        assert!((0.4..=2.5).contains(&start), "start out of range: {start}");
    }

    let _ = std::fs::remove_dir_all(&dir);
    let _ = Arc::new(());
}
