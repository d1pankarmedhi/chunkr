use chunkr::chunker::base::BaseChunker;
// Import the chunker module
use chunkr::chunker::char::CharacterChunker;
use chunkr::chunker::word::WordChunker;

#[test]
fn test_character_chunker() {
    let char_chunker = CharacterChunker::new();
    let input_text = "hello there how are you. I am fine thank you.";
    let chunk_size = 5;
    let expected = vec![
        "hello".to_string(),
        " ther".to_string(),
        "e how".to_string(),
        " are ".to_string(),
        "you.".to_string(),
        " I am".to_string(),
        " fine".to_string(),
        " than".to_string(),
        "k you".to_string(),
        ".".to_string(),
    ];
    let chunks = char_chunker.chunk_text(input_text, chunk_size);
    assert_eq!(chunks, expected);
}

#[test]
fn test_word_chunker() {
    let word_chunker = WordChunker::new();
    let input_text = "hello there how are you. I am fine thank you.";
    let chunk_size = 5;
    let expected = vec![
        "hello there how are you.".to_string(),
        "I am fine thank you.".to_string(),
    ];
    let chunks = word_chunker.chunk_text(input_text, chunk_size);
    assert_eq!(chunks, expected);
}
