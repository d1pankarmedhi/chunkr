use chunkr::chunker::{base::BaseChunker, word::WordChunker};
use chunkr::loader::base::BaseLoader;
use chunkr::loader::pdf::PDFLoader;
use std::env;

fn main() {
    let word_chunker = WordChunker::new();
    let loader = PDFLoader::new();

    let args: Vec<String> = env::args().collect();
    let chunk_size = args[1].parse().unwrap();
    let overlap = args[2].parse().unwrap();
    let path = &args[3];

    let input_text = loader.load_from_file(path).unwrap();

    let chunks = word_chunker
        .chunk_text(&input_text, chunk_size, overlap)
        .unwrap();
    let mut result: Vec<String> = Vec::new();
    for chunk in chunks {
        result.push(chunk.content.to_string());
    }
    print!("{:?}", result)
}
