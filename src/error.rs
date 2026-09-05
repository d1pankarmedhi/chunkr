use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq)]
pub enum ChunkrError {
    #[error("Empty input text provided")]
    EmptyInput,

    #[error("Invalid chunk size: {0} (must be > 0)")]
    InvalidChunkSize(usize),

    #[error(
        "Invalid chunk overlap: {overlap} (must be strictly less than chunk_size {chunk_size})"
    )]
    InvalidOverlap { chunk_size: usize, overlap: usize },

    #[error("Tokenizer error: {0}")]
    TokenizerError(String),

    #[error("Document parse error: {0}")]
    ParseError(String),

    #[error("IO error: {0}")]
    IoError(String),

    #[error("Generic error: {0}")]
    Generic(String),
}

impl From<String> for ChunkrError {
    fn from(s: String) -> Self {
        ChunkrError::Generic(s)
    }
}

impl From<&str> for ChunkrError {
    fn from(s: &str) -> Self {
        ChunkrError::Generic(s.to_string())
    }
}

impl From<std::io::Error> for ChunkrError {
    fn from(err: std::io::Error) -> Self {
        ChunkrError::IoError(err.to_string())
    }
}

impl From<lopdf::Error> for ChunkrError {
    fn from(err: lopdf::Error) -> Self {
        ChunkrError::ParseError(err.to_string())
    }
}
