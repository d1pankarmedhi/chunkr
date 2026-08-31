use serde_json::Value;
use std::collections::HashMap;

use crate::chunker::base::{BaseChunker, Chunker};
use crate::error::ChunkrError;
use crate::structures::document::Document;

/// Splits text into chunks by number of whitespace-delimited words.
#[derive(Debug, Clone)]
pub struct WordChunker {
    pub chunk_size: usize,
    pub overlap: usize,
}

impl WordChunker {
    /// Create a new WordChunker with default parameters (chunk_size: 200 words, overlap: 20 words)
    pub fn new() -> Self {
        Self {
            chunk_size: 200,
            overlap: 20,
        }
    }

    /// Builder method to specify word chunk size
    pub fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = chunk_size;
        self
    }

    /// Builder method to specify word overlap
    pub fn with_overlap(mut self, overlap: usize) -> Self {
        self.overlap = overlap;
        self
    }

    /// Helper to split text by word count
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

        let words: Vec<&str> = text.split_whitespace().collect();
        let total_words = words.len();

        if total_words == 0 {
            return Err(ChunkrError::EmptyInput);
        }

        let mut result = Vec::new();
        let step = chunk_size - overlap;
        let mut start_word = 0;
        let mut chunk_idx = 0;

        while start_word < total_words {
            let end_word = (start_word + chunk_size).min(total_words);
            let chunk_content = words[start_word..end_word].join(" ");

            let mut metadata = HashMap::new();
            metadata.insert("length".to_string(), Value::from(chunk_content.len()));
            metadata.insert("word_count".to_string(), Value::from(end_word - start_word));
            metadata.insert("start_word".to_string(), Value::from(start_word));
            metadata.insert("end_word".to_string(), Value::from(end_word));
            metadata.insert("chunk_index".to_string(), Value::from(chunk_idx));

            result.push(Document {
                content: chunk_content,
                metadata,
            });
            chunk_idx += 1;

            if end_word == total_words {
                break;
            }

            start_word += step;
        }

        Ok(result)
    }
}

impl Default for WordChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunker for WordChunker {
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError> {
        Self::split_text(text, self.chunk_size, self.overlap)
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for WordChunker {
    fn chunk_text(
        &self,
        text: &str,
        chunk_size: usize,
        overlap: usize,
    ) -> Result<Vec<Document>, String> {
        Self::split_text(text, chunk_size, overlap).map_err(|e| e.to_string())
    }
}
