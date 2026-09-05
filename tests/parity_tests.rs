use chunkr::chunker::ast_code::{AstCodeChunker, AstLanguage};
use chunkr::chunker::base::BaseChunker;

fn oversized_rust_function() -> String {
    let mut code = String::from("fn big_function() {\n");
    for i in 0..200 {
        code.push_str(&format!(
            "    let value_{} = compute_something({});\n",
            i, i
        ));
    }
    code.push_str("}\n");
    code
}

#[test]
fn test_ast_chunk_text_syncs_sub_chunker_budget() {
    // Regression: chunk_text used to set max_chunk_size but left the inner
    // recursive sub-chunker at the stale 1500-char budget, so oversized
    // definitions were sub-split with the wrong window.
    let code = oversized_rust_function();
    assert!(code.len() > 1500, "fixture must exceed one definition");

    let chunker = AstCodeChunker::new(AstLanguage::Rust);
    let chunks = chunker
        .chunk_text(&code, 300, 30)
        .expect("chunk_text should succeed");

    assert!(
        chunks.len() > 1,
        "oversized function should sub-split, got {} chunk(s)",
        chunks.len()
    );
    for chunk in &chunks {
        assert_eq!(
            chunk.metadata.get("language").and_then(|v| v.as_str()),
            Some("rust")
        );
        assert!(
            chunk.content.len() <= 450,
            "chunk exceeds budget+slack: {} chars",
            chunk.content.len()
        );
    }
}
