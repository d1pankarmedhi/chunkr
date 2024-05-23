use chunkr::chunker::base::BaseChunker;
// Import the chunker module
use chunkr::chunker::char::CharacterChunker;
use chunkr::chunker::word::WordChunker;
use chunkr::loader::base::BaseLoader;
use chunkr::loader::pdf::PDFLoader;

#[test]
fn test_character_chunker() {
    let char_chunker = CharacterChunker::new();
    let loader = PDFLoader::new();
    let input_text = loader
        .load_from_file("tests/test_files/sample_doc.pdf")
        .unwrap();
    let chunk_size = 1000;
    let overlap = 50;
    let chunks = char_chunker
        .chunk_text(&input_text, chunk_size, overlap)
        .unwrap();
    dbg!(chunks.len());
    assert_eq!(5, chunks.len());
}

#[test]
fn test_word_chunker() {
    let word_chunker = WordChunker::new();
    let loader = PDFLoader::new();
    let input_text = loader
        .load_from_file("tests/test_files/sample_doc.pdf")
        .unwrap();

    let chunk_size = 500;
    let overlap = 50;
    let chunks = word_chunker
        .chunk_text(&input_text, chunk_size, overlap)
        .unwrap();
    dbg!(&chunks);
    assert_eq!(1627, chunks[0].content.len());
}
