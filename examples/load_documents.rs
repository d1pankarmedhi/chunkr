//! Format-aware loading for spreadsheets, office documents, books and mail.
//!
//! Run with `cargo run --example load_documents -- <path>`, or with no argument
//! to print the extension registry.

use chunkr::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1);

    let Some(path) = path else {
        println!(
            "Supported extensions: {}",
            supported_extensions().join(", ")
        );
        return Ok(());
    };

    // Auto-dispatch on the file extension.
    let documents = load_documents(&path)?;
    println!("Loaded {} document(s) from {}", documents.len(), path);

    let chunker = RecursiveChunker::new()
        .with_chunk_size(500)
        .with_overlap(50);
    let chunks = chunker.chunk_documents(&documents)?;
    println!("Generated {} chunks", chunks.len());

    if let Some(first) = chunks.first() {
        println!("\nFirst chunk metadata: {:?}", first.metadata);
        println!("First chunk preview: {}", first.content);
    }

    // Explicit extractors are available when a caller knows the format.
    let bytes = std::fs::read("tests/test_files/sample_doc.pdf")?;
    let pages = PDFLoader::new().load_pages_from_bytes(&bytes)?;
    println!("\nPDF pages: {}", pages.len());

    Ok(())
}
