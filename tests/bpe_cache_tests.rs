use chunkr::prelude::*;

const SAMPLE: &str = "Large language models require accurate token counts for prompt window management. Retrieval pipelines split documents into chunks that fit the model context. ";

#[test]
fn test_shared_bpe_identical_output() {
    let a = TokenChunker::with_encoding(50, 10, TokenEncoding::Cl100kBase).unwrap();
    let b = TokenChunker::with_encoding(50, 10, TokenEncoding::Cl100kBase).unwrap();
    let chunks_a = a.chunk(SAMPLE).unwrap();
    let chunks_b = b.chunk(SAMPLE).unwrap();
    assert_eq!(chunks_a.len(), chunks_b.len());
    for (ca, cb) in chunks_a.iter().zip(chunks_b.iter()) {
        assert_eq!(ca.content, cb.content);
        assert_eq!(ca.metadata, cb.metadata);
    }
    // Cloned chunkers share the same tables and behave identically.
    let c = a.clone();
    let chunks_c = c.chunk(SAMPLE).unwrap();
    assert_eq!(chunks_c.len(), chunks_a.len());
    for (ca, cc) in chunks_a.iter().zip(chunks_c.iter()) {
        assert_eq!(ca.content, cc.content);
    }
}

#[test]
fn test_all_encodings_construct_and_chunk() {
    let encodings = [
        TokenEncoding::Cl100kBase,
        TokenEncoding::O200kBase,
        TokenEncoding::P50kBase,
        TokenEncoding::R50kBase,
    ];
    for enc in encodings {
        let chunker = TokenChunker::with_encoding(20, 5, enc).unwrap();
        let chunks = chunker.chunk("Hello world, this is a short test.").unwrap();
        assert!(!chunks.is_empty());
        assert_eq!(
            chunker.count_tokens("Hello world"),
            TokenChunker::with_encoding(20, 5, enc)
                .unwrap()
                .count_tokens("Hello world")
        );
    }
}

#[test]
fn test_invalid_configs_still_error() {
    assert!(TokenChunker::with_encoding(0, 0, TokenEncoding::Cl100kBase).is_err());
    assert!(TokenChunker::with_encoding(50, 50, TokenEncoding::Cl100kBase).is_err());
    assert!(TokenChunker::with_encoding(50, 60, TokenEncoding::Cl100kBase).is_err());
    assert!(TokenChunker::new().is_ok());
}
