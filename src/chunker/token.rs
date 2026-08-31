use std::collections::HashMap;
use serde_json::Value;
use tiktoken_rs::{cl100k_base, o200k_base, p50k_base, r50k_base, CoreBPE};

use crate::chunker::base::{BaseChunker, Chunker};
use crate::error::ChunkrError;
use crate::structures::document::Document;

/// Supported OpenAI BPE tokenizer encodings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TokenEncoding {
    #[default]
    Cl100kBase,
    O200kBase,
    P50kBase,
    R50kBase,
}

impl TokenEncoding {
    pub fn get_bpe(&self) -> Result<CoreBPE, ChunkrError> {
        match self {
            TokenEncoding::Cl100kBase => cl100k_base().map_err(|e| ChunkrError::TokenizerError(e.to_string())),
            TokenEncoding::O200kBase => o200k_base().map_err(|e| ChunkrError::TokenizerError(e.to_string())),
            TokenEncoding::P50kBase => p50k_base().map_err(|e| ChunkrError::TokenizerError(e.to_string())),
            TokenEncoding::R50kBase => r50k_base().map_err(|e| ChunkrError::TokenizerError(e.to_string())),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            TokenEncoding::Cl100kBase => "cl100k_base",
            TokenEncoding::O200kBase => "o200k_base",
            TokenEncoding::P50kBase => "p50k_base",
            TokenEncoding::R50kBase => "r50k_base",
        }
    }
}

/// Splits text into chunks by token count using fast BPE tokenizers.
pub struct TokenChunker {
    pub chunk_size: usize,
    pub overlap: usize,
    pub encoding: TokenEncoding,
    bpe: CoreBPE,
}

impl std::fmt::Debug for TokenChunker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenChunker")
            .field("chunk_size", &self.chunk_size)
            .field("overlap", &self.overlap)
            .field("encoding", &self.encoding)
            .finish()
    }
}

impl Clone for TokenChunker {
    fn clone(&self) -> Self {
        Self::with_encoding(self.chunk_size, self.overlap, self.encoding)
            .expect("Valid BPE clone")
    }
}

impl TokenChunker {
    /// Create a new TokenChunker with cl100k_base tokenizer (default: chunk_size=512, overlap=50 tokens)
    pub fn new() -> Result<Self, ChunkrError> {
        Self::with_encoding(512, 50, TokenEncoding::Cl100kBase)
    }

    /// Create with specific chunk size, overlap, and token encoding
    pub fn with_encoding(
        chunk_size: usize,
        overlap: usize,
        encoding: TokenEncoding,
    ) -> Result<Self, ChunkrError> {
        if chunk_size == 0 {
            return Err(ChunkrError::InvalidChunkSize(0));
        }
        if overlap >= chunk_size {
            return Err(ChunkrError::InvalidOverlap { chunk_size, overlap });
        }
        let bpe = encoding.get_bpe()?;
        Ok(Self {
            chunk_size,
            overlap,
            encoding,
            bpe,
        })
    }

    /// Builder for chunk size
    pub fn with_chunk_size(mut self, chunk_size: usize) -> Result<Self, ChunkrError> {
        if chunk_size == 0 {
            return Err(ChunkrError::InvalidChunkSize(0));
        }
        if self.overlap >= chunk_size {
            return Err(ChunkrError::InvalidOverlap {
                chunk_size,
                overlap: self.overlap,
            });
        }
        self.chunk_size = chunk_size;
        Ok(self)
    }

    /// Builder for overlap
    pub fn with_overlap(mut self, overlap: usize) -> Result<Self, ChunkrError> {
        if overlap >= self.chunk_size {
            return Err(ChunkrError::InvalidOverlap {
                chunk_size: self.chunk_size,
                overlap,
            });
        }
        self.overlap = overlap;
        Ok(self)
    }

    /// Count tokens in text
    pub fn count_tokens(&self, text: &str) -> usize {
        self.bpe.encode_ordinary(text).len()
    }
}

impl Chunker for TokenChunker {
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError> {
        if text.trim().is_empty() {
            return Err(ChunkrError::EmptyInput);
        }

        let tokens = self.bpe.encode_ordinary(text);
        let total_tokens = tokens.len();

        if total_tokens == 0 {
            return Err(ChunkrError::EmptyInput);
        }

        let step = self.chunk_size - self.overlap;
        let mut result = Vec::with_capacity((total_tokens / step) + 1);
        let mut start_token = 0;
        let mut chunk_idx = 0;

        while start_token < total_tokens {
            let end_token = (start_token + self.chunk_size).min(total_tokens);
            let token_slice = &tokens[start_token..end_token];

            let decoded_text = self
                .bpe
                .decode(token_slice.to_vec())
                .map_err(|e| ChunkrError::TokenizerError(e.to_string()))?;

            let trimmed = decoded_text.trim();
            if !trimmed.is_empty() {
                let mut metadata = HashMap::with_capacity(4);
                metadata.insert("token_count".to_string(), Value::from(token_slice.len()));
                metadata.insert("length".to_string(), Value::from(trimmed.len()));
                metadata.insert("chunk_index".to_string(), Value::from(chunk_idx));
                metadata.insert("encoding".to_string(), Value::from(self.encoding.as_str()));

                result.push(Document {
                    content: trimmed.to_string(),
                    metadata,
                });
                chunk_idx += 1;
            }

            if end_token == total_tokens {
                break;
            }

            start_token += step;
        }

        Ok(result)
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for TokenChunker {
    fn chunk_text(
        &self,
        text: &str,
        chunk_size: usize,
        overlap: usize,
    ) -> Result<Vec<Document>, String> {
        let chunker = TokenChunker::with_encoding(chunk_size, overlap, self.encoding)
            .map_err(|e| e.to_string())?;
        chunker.chunk(text).map_err(|e| e.to_string())
    }
}
