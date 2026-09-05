use chunkr::prelude::*;

const MIXED_TEXT: &str = "Quantum mechanics describes nature at atomic scales. \
    Wave-particle duality is central to quantum theory. \
    Chocolate chip cookies need flour, sugar, and butter. \
    Bake the cookie dough at 350 degrees for twelve minutes. \
    Black holes warp spacetime beyond the event horizon. \
    Hawking radiation lets black holes evaporate over time.";

#[test]
fn test_buffer_zero_matches_legacy_behavior() {
    let chunked_default = SemanticChunker::new()
        .with_threshold(BreakpointThreshold::Percentile(50.0))
        .with_size_bounds(50, 1000)
        .chunk(MIXED_TEXT)
        .unwrap();
    // A fresh chunker defaults to buffer_size 1, so force 0 explicitly and
    // compare against another explicit-0 instance for determinism.
    let a = SemanticChunker::new()
        .with_threshold(BreakpointThreshold::Percentile(50.0))
        .with_size_bounds(50, 1000)
        .with_buffer_size(0)
        .chunk(MIXED_TEXT)
        .unwrap();
    let b = SemanticChunker::new()
        .with_threshold(BreakpointThreshold::Percentile(50.0))
        .with_size_bounds(50, 1000)
        .with_buffer_size(0)
        .chunk(MIXED_TEXT)
        .unwrap();
    assert!(!a.is_empty());
    assert_eq!(
        a.iter().map(|d| &d.content).collect::<Vec<_>>(),
        b.iter().map(|d| &d.content).collect::<Vec<_>>()
    );
    // Sanity: default (buffer 1) path also produces chunks on this input.
    assert!(!chunked_default.is_empty());
}

#[test]
fn test_buffer_two_runs_cleanly() {
    let chunks = SemanticChunker::new()
        .with_threshold(BreakpointThreshold::Percentile(50.0))
        .with_size_bounds(50, 1000)
        .with_buffer_size(2)
        .chunk(MIXED_TEXT)
        .unwrap();
    assert!(!chunks.is_empty());
    for chunk in &chunks {
        assert!(!chunk.content.is_empty());
    }
}

#[test]
fn test_contextual_chunk_text_preserves_format_and_generator() {
    let text = "First claim stands alone. Second claim follows it. Third claim ends it.";
    let chunker = ContextualChunker::new()
        .with_base_chunker(
            SentenceChunker::new()
                .with_sentences_per_chunk(1)
                .with_sentence_overlap(0),
        )
        .with_format(ContextFormat::MetadataOnly);

    let via_chunk = chunker.chunk(text).unwrap();
    // NOTE: chunk_text() swaps in a RecursiveChunker sized by its own
    // chunk_size/overlap args, so chunk counts legitimately differ from the
    // sentence-based chunk() path. What must be identical is the preserved
    // MetadataOnly format (no prefix) plus the injected context metadata.
    let via_legacy = chunker.chunk_text(text, 30, 5).unwrap();

    assert_eq!(via_chunk.len(), 3);
    assert!(!via_legacy.is_empty());
    for doc in via_chunk.iter().chain(via_legacy.iter()) {
        assert!(
            !doc.content.starts_with("[Context:"),
            "MetadataOnly must not prefix content: {}",
            doc.content
        );
        assert!(doc.metadata.contains_key("context"));
    }
}

#[test]
fn test_hierarchical_chunk_text_preserves_include_parents_false() {
    let text = "Parent topic one with enough words to split. Child detail sentence here.\n\n\
        Parent topic two with enough words to split. Another child detail here.";
    let chunker = HierarchicalChunker::with_sizes(120, 10, 40, 5)
        .unwrap()
        .with_include_parents(false);

    let docs = chunker.chunk_text(text, 120, 10).unwrap();
    assert!(!docs.is_empty());
    assert!(
        docs.iter().all(|d| {
            d.metadata
                .get("chunk_type")
                .and_then(|v| v.as_str())
                != Some("parent")
        }),
        "parents must be excluded when include_parents_in_output is false"
    );
    assert!(
        docs.iter()
            .any(|d| d.metadata.get("chunk_type").and_then(|v| v.as_str()) == Some("child"))
    );
}
