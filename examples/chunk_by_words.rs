use chunkr::chunker::{base::BaseChunker, word::WordChunker};
use std::env;

fn main() {
    let word_chunker = WordChunker::new();
    let args: Vec<String> = env::args().collect();

    let chunk_size = args[1].parse().unwrap();
    let input_text = &args[2];
    let chunks = word_chunker.chunk_text(input_text, chunk_size);
    println!("{:?}", chunks);
}
