use chunkr::chunker::{base::BaseChunker, word::WordChunker};
use std::env;

fn main() {
    let word_chunker = WordChunker::new();
    let args: Vec<String> = env::args().collect();

    let chunk_size = args[1].parse().unwrap();
    let overlap = args[2].parse().unwrap();

    let input_text = &args[3];
    let chunks = word_chunker
        .chunk_text(input_text, chunk_size, overlap)
        .unwrap();

    // get content from documents
    let mut result: Vec<String> = Vec::new();
    for chunk in chunks {
        result.push(chunk.content.to_string());
    }
    print!("{:?}", result)
}
