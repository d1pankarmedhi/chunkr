//! Subtitle/caption coverage: SRT and WebVTT parsing, time-window chunking with
//! monotonic spans, format dispatch and routing.

use std::path::PathBuf;

use chunkr::prelude::*;

const SRT: &str = "1\n00:00:01,000 --> 00:00:03,500\n{\\an8}Hello there.\n\n2\n00:00:03,600 --> 00:00:06,000\n<i>Second</i> line.\n\n3\n00:00:06,100 --> 00:00:09,000\nThird &amp; final cue.\n";

const VTT: &str = "WEBVTT\nKind: captions\n\nNOTE this block is not a cue\n\n00:00.000 --> 00:02.000 align:start position:0%\n<v Roger Bingham>We are in New York City.\n\n00:02.000 --> 00:04.500\n<v Roger Bingham>Just <c.yellow>arrived</c>.\n\n00:04.500 --> 00:06.000\n<v Neil deGrasse Tyson>Different speaker here.\n";

fn unique_tmp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "chunkr_subtitles_{}_{}_{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp test dir");
    dir
}

fn seconds(document: &Document, key: &str) -> f64 {
    document
        .metadata
        .get(key)
        .and_then(|value| value.as_f64())
        .unwrap_or_else(|| panic!("missing numeric metadata: {key}"))
}

#[test]
fn test_srt_cues_are_parsed_with_positions_and_entities() {
    let cues = parse_cues(SRT).unwrap();

    assert_eq!(cues.len(), 3);
    assert_eq!(cues[0].start_ms, 1_000);
    assert_eq!(cues[0].end_ms, 3_500);
    assert_eq!(
        cues[0].text, "Hello there.",
        "SRT position group must be dropped"
    );
    assert_eq!(cues[1].text, "Second line.", "inline tags must be dropped");
    assert_eq!(
        cues[2].text, "Third & final cue.",
        "entities must be decoded"
    );
}

#[test]
fn test_webvtt_skips_metadata_blocks_and_captures_speakers() {
    let cues = parse_cues(VTT).unwrap();

    assert_eq!(cues.len(), 3, "NOTE block must not become a cue");
    assert_eq!(cues[0].start_ms, 0, "VTT may omit the hours field");
    assert_eq!(cues[0].end_ms, 2_000);
    assert_eq!(cues[0].text, "We are in New York City.");
    assert_eq!(cues[0].speaker.as_deref(), Some("Roger Bingham"));
    assert_eq!(cues[1].text, "Just arrived.");
    assert_eq!(cues[2].speaker.as_deref(), Some("Neil deGrasse Tyson"));
    assert_eq!(
        cues[2].end_ms, 6_000,
        "cue settings must not break the end time"
    );
}

#[test]
fn test_windows_never_split_a_cue_and_spans_stay_monotonic() {
    let chunker = SubtitleChunker::new().with_max_seconds(2.0);
    let chunks = chunker.chunk(VTT).unwrap();

    assert!(chunks.len() > 1, "a 6s track at 2s windows must split");

    let mut previous_end = -1.0;
    for chunk in &chunks {
        let start = seconds(chunk, "start_time_s");
        let end = seconds(chunk, "end_time_s");
        assert!(start >= previous_end, "windows must not go backwards");
        assert!(end > start, "a window must have positive duration");
        previous_end = end;
    }

    // Every cue text lands in exactly one window, whole.
    let joined: Vec<&str> = chunks.iter().map(|chunk| chunk.content.as_str()).collect();
    let all = joined.join("\n");
    assert!(all.contains("We are in New York City."));
    assert!(all.contains("Just arrived."));
    assert!(all.contains("Different speaker here."));
    assert_eq!(
        joined
            .iter()
            .filter(|text| text.contains("Just arrived."))
            .count(),
        1,
        "a cue must not be duplicated across windows"
    );
}

#[test]
fn test_window_budgets_and_speaker_metadata() {
    let chunks = SubtitleChunker::new()
        .with_max_seconds(2.5)
        .with_max_characters(1000)
        .chunk(VTT)
        .unwrap();

    assert_eq!(chunks.len(), 3, "one cue per window at 2.5s");
    assert_eq!(
        chunks[0].metadata.get("speaker").unwrap(),
        &serde_json::json!("Roger Bingham")
    );
    assert_eq!(
        chunks[0].metadata.get("cue_count").unwrap(),
        &serde_json::json!(1)
    );
    assert_eq!(seconds(&chunks[0], "start_time_s"), 0.0);
    assert_eq!(seconds(&chunks[0], "duration_s"), 2.0);
    assert_eq!(
        chunks[2].metadata.get("speaker").unwrap(),
        &serde_json::json!("Neil deGrasse Tyson")
    );

    // A speaker change ends the window even with budget to spare: a chunk that
    // mixes two voices cannot be attributed, so two windows are better than one.
    let turns = SubtitleChunker::new()
        .with_max_seconds(10.0)
        .chunk(VTT)
        .unwrap();
    assert_eq!(turns.len(), 2, "one window per speaker turn");
    assert_eq!(
        turns[0].metadata.get("cue_count").unwrap(),
        &serde_json::json!(2),
        "both Roger cues stay together"
    );
    assert_eq!(
        turns[0].metadata.get("speaker").unwrap(),
        &serde_json::json!("Roger Bingham")
    );
    assert_eq!(
        turns[1].metadata.get("speaker").unwrap(),
        &serde_json::json!("Neil deGrasse Tyson")
    );
    assert!(seconds(&turns[1], "start_time_s") >= seconds(&turns[0], "end_time_s"));

    // Only an unlabelled cue lets two speakers share a window; then the window
    // records who and how many turns it contains.
    let mixed_track = "1\n00:00:00,000 --> 00:00:01,000\n<v A>Yes.\n\n2\n00:00:01,000 --> 00:00:02,000\nNo.\n\n3\n00:00:02,000 --> 00:00:03,000\n<v B>Never.\n";
    let mixed = SubtitleChunker::new()
        .with_max_seconds(10.0)
        .chunk(mixed_track);
    let mixed = mixed.expect("chunked");
    assert_eq!(mixed.len(), 1);
    assert_eq!(
        mixed[0].metadata.get("speakers").unwrap(),
        &serde_json::json!("A, B")
    );
    assert_eq!(
        mixed[0].metadata.get("speaker_turns").unwrap(),
        &serde_json::json!(2)
    );
    assert!(
        !mixed[0].metadata.contains_key("speaker"),
        "mixed windows carry no single speaker: {:?}",
        mixed[0].metadata
    );
}

#[test]
fn test_character_budget_closes_the_window() {
    let chunks = SubtitleChunker::new()
        .with_max_seconds(600.0)
        .with_max_characters(25)
        .chunk(SRT)
        .unwrap();

    assert!(chunks.len() > 1, "char budget must close windows");
    for chunk in &chunks {
        assert!(
            chunk.content.len() <= 25
                || chunk.metadata.get("cue_count") == Some(&serde_json::json!(1)),
            "only a single over-long cue may exceed the budget: {} chars",
            chunk.content.len()
        );
    }
}

#[test]
fn test_single_long_cue_is_atomic_and_terminates() {
    let long_cue = format!(
        "1\n00:00:01,000 --> 00:10:00,000\n{}\n",
        "word ".repeat(400)
    );
    let chunks = SubtitleChunker::new()
        .with_max_seconds(1.0)
        .with_max_characters(10)
        .chunk(&long_cue)
        .unwrap();

    assert_eq!(chunks.len(), 1, "a single cue is never split");
    assert_eq!(seconds(&chunks[0], "start_time_s"), 1.0);
    assert_eq!(seconds(&chunks[0], "end_time_s"), 600.0);
}

#[test]
fn test_invalid_subtitle_input_errors() {
    assert!(matches!(
        SubtitleChunker::new().chunk(""),
        Err(ChunkrError::EmptyInput)
    ));

    let error = SubtitleChunker::new()
        .chunk("plain prose, not a caption track")
        .unwrap_err();
    assert!(
        error.to_string().contains("no cues found"),
        "expected a cue parse error, got: {error}"
    );
}

#[test]
fn test_loader_returns_one_document_per_cue_with_spans() {
    let documents = load_documents_from_bytes(SRT.as_bytes(), "srt", Some("talk.srt")).unwrap();

    assert_eq!(documents.len(), 3);
    let first = &documents[0];
    assert_eq!(first.content, "Hello there.");
    assert!(!first.content.contains("-->"), "timestamps leave the text");
    assert_eq!(
        first.metadata.get("format").unwrap(),
        &serde_json::json!("subtitle")
    );
    assert_eq!(
        first.metadata.get("subtitle_format").unwrap(),
        &serde_json::json!("srt")
    );
    assert_eq!(
        first.metadata.get("cue_index").unwrap(),
        &serde_json::json!(0)
    );
    assert_eq!(
        first.metadata.get("total_cues").unwrap(),
        &serde_json::json!(3)
    );
    assert_eq!(
        first.metadata.get("file_name").unwrap(),
        &serde_json::json!("talk.srt")
    );
    assert_eq!(seconds(first, "start_time_s"), 1.0);
    assert_eq!(seconds(first, "end_time_s"), 3.5);

    let last = &documents[2];
    assert_eq!(
        last.metadata.get("cue_index").unwrap(),
        &serde_json::json!(2)
    );
    assert_eq!(last.content, "Third & final cue.");
    assert!(supported_extensions().iter().any(|e| e == "vtt"));
}

#[test]
fn test_cue_documents_merge_into_windows_with_identity_metadata() {
    let documents = load_documents_from_bytes(VTT.as_bytes(), "vtt", Some("talk.vtt")).unwrap();
    assert_eq!(documents.len(), 3);

    let chunks = SubtitleChunker::new()
        .with_max_seconds(2.5)
        .chunk_documents(&documents)
        .unwrap();

    assert_eq!(chunks.len(), 3, "one cue per window at 2.5s");
    for (index, chunk) in chunks.iter().enumerate() {
        assert_eq!(
            chunk.metadata.get("chunk_index").unwrap(),
            &serde_json::json!(index)
        );
        assert_eq!(
            chunk.metadata.get("file_name").unwrap(),
            &serde_json::json!("talk.vtt"),
            "identity metadata must survive merging"
        );
        assert_eq!(
            chunk.metadata.get("total_cues").unwrap(),
            &serde_json::json!(3)
        );
        assert!(
            !chunk.metadata.contains_key("cue_index"),
            "a window must not claim one cue index"
        );
    }
    assert_eq!(seconds(&chunks[0], "start_time_s"), 0.0);
    assert_eq!(seconds(&chunks[0], "end_time_s"), 2.0);
    assert_eq!(seconds(&chunks[2], "start_time_s"), 4.5);
}

#[test]
fn test_legacy_encoded_subtitles_are_decoded() {
    let latin1 = b"1\n00:00:01,000 --> 00:00:02,000\nCaf\xe9 cr\xe8me\n";
    let documents = load_documents_from_bytes(latin1, "vtt", None).unwrap();
    assert!(documents[0].content.contains("Café crème"));
}

#[test]
fn test_directory_loader_routes_subtitles_to_time_windows() {
    let dir = unique_tmp_dir("route");
    std::fs::write(dir.join("talk.vtt"), VTT).unwrap();

    let chunks = DirectoryLoader::new()
        .with_chunk_size(60)
        .with_overlap(0)
        .load_and_chunk(&dir)
        .unwrap();

    assert!(!chunks.is_empty());
    for chunk in &chunks {
        assert!(
            chunk
                .metadata
                .get("start_time_s")
                .and_then(|v| v.as_f64())
                .is_some(),
            "every routed subtitle chunk needs a start span: {:?}",
            chunk.metadata
        );
        assert_eq!(
            chunk.metadata.get("file_extension").unwrap(),
            &serde_json::json!("vtt")
        );
        assert!(
            chunk.metadata.get("cue_count").unwrap().is_number(),
            "window cue counts survive routing"
        );
    }

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_packing_never_inherits_a_wrong_time_span() {
    let chunks = SubtitleChunker::new()
        .with_max_seconds(2.0)
        .chunk(VTT)
        .unwrap();
    assert!(chunks.len() > 1);

    let packed = ChunkPacker::new(10_000).pack(&chunks);

    assert_eq!(packed.len(), 1);
    let metadata = &packed[0].metadata;
    assert!(
        metadata.get("start_time_s").is_none() && metadata.get("end_time_s").is_none(),
        "a merged chunk must not claim one source's span: {metadata:?}"
    );
    assert!(metadata.contains_key("merged_start_time_s"));
    assert!(
        metadata.get("cue_count") == Some(&serde_json::json!(3)),
        "merged cue counts are summed: {metadata:?}"
    );
}
