pub trait BaseChunker<O> {
    // generic return type
    fn chunk_text(&self, text: &str, chunk_sizse: usize) -> O;
}
