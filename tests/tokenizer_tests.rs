use chunkr::prelude::*;

const PARAGRAPH: &str =
    "Retrieval-Augmented Generation grounds large language models on external knowledge. \
    Chunking splits documents into semantically coherent segments for vector indexing. \
    High-performance chunking preserves sentence and paragraph boundaries without excess latency. ";

#[test]
fn test_all_encodings_round_trip() {
    let encodings = [
        TokenEncoding::Cl100kBase,
        TokenEncoding::O200kBase,
        TokenEncoding::P50kBase,
        TokenEncoding::R50kBase,
    ];
    for enc in encodings {
        let chunker = TokenChunker::with_encoding(50, 10, enc).unwrap();
        let chunks = chunker.chunk(PARAGRAPH).unwrap();
        assert!(!chunks.is_empty(), "no chunks for {:?}", enc);
        for chunk in &chunks {
            // Re-encoding the decoded content must (nearly) reproduce the
            // recorded count. Exact equality is NOT guaranteed: chunks are
            // trimmed after decode, and dropping a boundary space can merge
            // or split one BPE token (verified this ±1 drift also occurs on
            // tiktoken-rs 0.6, so it is inherent trim behavior, not a
            // regression). A drift > 1 would signal an encode/decode break.
            let recount = chunker.count_tokens(&chunk.content);
            let recorded = chunk
                .metadata
                .get("token_count")
                .and_then(|v| v.as_u64())
                .expect("token_count metadata") as usize;
            assert!(
                recount.abs_diff(recorded) <= 1,
                "round-trip drift for {:?}: recount={} recorded={} chunk {:?}",
                enc,
                recount,
                recorded,
                chunk.content
            );
            assert!(recorded > 0, "empty token span for {:?}", enc);
        }
    }
}

#[test]
fn test_late_spans_advance_on_repeated_text() {
    // One sentence repeated: overlapping windows produce same-content chunks
    // that must map to successive occurrences, not all to the first.
    let sentence = "The quick brown fox jumps over the lazy dog. ";
    let text = sentence.repeat(4);

    let base = RecursiveChunker::new().with_chunk_size(60).with_overlap(15);
    let late = LateChunker::new().with_base_chunker(base);
    let chunks = late.chunk(&text).unwrap();
    assert!(
        chunks.len() >= 2,
        "expected several chunks, got {}",
        chunks.len()
    );

    let starts: Vec<u64> = chunks
        .iter()
        .map(|c| {
            c.metadata
                .get("char_start")
                .and_then(|v| v.as_u64())
                .expect("char_start metadata")
        })
        .collect();
    for w in starts.windows(2) {
        assert!(
            w[1] > w[0],
            "char_start must strictly increase on repeated text, got {:?}",
            starts
        );
    }

    let tok_starts: Vec<u64> = chunks
        .iter()
        .map(|c| {
            c.metadata
                .get("token_start")
                .and_then(|v| v.as_u64())
                .expect("token_start metadata")
        })
        .collect();
    for w in tok_starts.windows(2) {
        assert!(
            w[1] >= w[0],
            "token_start must be monotonic, got {:?}",
            tok_starts
        );
    }
}

#[test]
fn test_encode_throughput_probe() {
    // Print-only probe: no timing assert (machine-dependent). Run with
    // `cargo test --release -- --nocapture` and read the number.
    let para = "Retrieval-Augmented Generation (RAG) is an AI framework for retrieving facts. ";
    let text = format!("{}\n\n", para.repeat(40)).repeat(60);
    assert!(text.len() > 150_000, "corpus too small: {}", text.len());

    let chunker = TokenChunker::with_encoding(512, 50, TokenEncoding::Cl100kBase).unwrap();
    // Warmup (BPE cache + allocator).
    let _ = chunker.count_tokens(&text);

    let iters = 5;
    let start = std::time::Instant::now();
    let mut total_tokens = 0;
    for _ in 0..iters {
        total_tokens += chunker.count_tokens(&text);
    }
    let elapsed = start.elapsed();
    println!(
        "ENCODE-PROBE bytes={} tokens_per_iter={} iters={} total_ms={:.1} ms_per_iter={:.1}",
        text.len(),
        total_tokens / iters,
        iters,
        elapsed.as_secs_f64() * 1000.0,
        elapsed.as_secs_f64() * 1000.0 / iters as f64,
    );
}
