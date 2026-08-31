use std::collections::HashMap;
use serde_json::Value;

use crate::chunker::base::{BaseChunker, Chunker};
use crate::error::ChunkrError;
use crate::structures::document::Document;

/// Splits text into chunks by a fixed number of Unicode characters with optional overlap.
#[derive(Debug, Clone)]
pub struct CharacterChunker {
    pub chunk_size: usize,
    pub overlap: usize,
}

impl CharacterChunker {
    /// Create a new CharacterChunker with default parameters (chunk_size: 1000, overlap: 200)
    pub fn new() -> Self {
        Self {
            chunk_size: 1000,
            overlap: 200,
        }
    }

    /// Builder method to specify chunk size
    pub fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = chunk_size;
        self
    }

    /// Builder method to specify overlap
    pub fn with_overlap(mut self, overlap: usize) -> Self {
        self.overlap = overlap;
        self
    }

    /// Helper to split text using given chunk_size and overlap
    pub fn split_text(text: &str, chunk_size: usize, overlap: usize) -> Result<Vec<Document>, ChunkrError> {
        if text.trim().is_empty() {
            return Err(ChunkrError::EmptyInput);
        }
        if chunk_size == 0 {
            return Err(ChunkrError::InvalidChunkSize(0));
        }
        if overlap >= chunk_size {
            return Err(ChunkrError::InvalidOverlap { chunk_size, overlap });
        }

        // Collect character byte offsets for zero-copy slicing
        let char_indices: Vec<(usize, char)> = text.char_indices().collect();
        let total_chars = char_indices.len();

        if total_chars == 0 {
            return Err(ChunkrError::EmptyInput);
        }

        let mut result = Vec::new();
        let step = chunk_size - overlap;
        let mut start_char = 0;
        let mut chunk_idx = 0;

        while start_char < total_chars {
            let end_char = (start_char + chunk_size).min(total_chars);
            let start_byte = char_indices[start_char].0;
            let end_byte = if end_char < total_chars {
                char_indices[end_char].0
            } else {
                text.len()
            };

            let chunk_str = &text[start_byte..end_byte];
            let trimmed = chunk_str.trim();

            if !trimmed.is_empty() {
                let mut metadata = HashMap::new();
                metadata.insert("length".to_string(), Value::from(trimmed.len()));
                metadata.insert("char_count".to_string(), Value::from(end_char - start_char));
                metadata.insert("start_char".to_string(), Value::from(start_char));
                metadata.insert("end_char".to_string(), Value::from(end_char));
                metadata.insert("chunk_index".to_string(), Value::from(chunk_idx));

                result.push(Document {
                    content: trimmed.to_string(),
                    metadata,
                });
                chunk_idx += 1;
            }

            if end_char == total_chars {
                break;
            }

            start_char += step;
        }

        Ok(result)
    }
}

impl Default for CharacterChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunker for CharacterChunker {
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError> {
        Self::split_text(text, self.chunk_size, self.overlap)
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for CharacterChunker {
    fn chunk_text(
        &self,
        text: &str,
        chunk_size: usize,
        overlap: usize,
    ) -> Result<Vec<Document>, String> {
        Self::split_text(text, chunk_size, overlap).map_err(|e| e.to_string())
    }
}
