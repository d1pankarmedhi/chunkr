use chunkr::prelude::*;
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let path = if args.len() > 1 {
        &args[1]
    } else {
        "tests/test_files/sample_doc.pdf"
    };

    let loader = PDFLoader::new();
    println!("Loading PDF from: {}", path);

    // 1. Load full document text
    let full_text = loader.load_from_file(path)?;
    println!("Extracted {} characters of text.", full_text.len());

    // 2. Load pages as structured Documents
    let pages = loader.load_pages_from_file(path)?;
    println!("Extracted {} pages.", pages.len());

    // 3. Chunk pages using RecursiveChunker
    let chunker = RecursiveChunker::new()
        .with_chunk_size(500)
        .with_overlap(50);
    let chunks = chunker.chunk_documents(&pages)?;
    println!("Generated {} chunks across all pages.", chunks.len());

    if let Some(first) = chunks.first() {
        println!("\nFirst chunk preview:");
        println!("Content: {}", first.content);
        println!("Metadata: {:?}", first.metadata);
    }

    Ok(())
}
