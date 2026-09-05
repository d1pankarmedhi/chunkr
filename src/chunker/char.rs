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

        let mut result = Vec::new();
        let step = chunk_size - overlap;

        // Char count (allocation-free) so we can derive chunk windows first.
        let total_chars = text.chars().count();
        if total_chars == 0 {
            return Err(ChunkrError::EmptyInput);
        }

        // Byte offsets are only needed at chunk start/end char positions
        // (2 per chunk), not at every char. Collect those in a single
        // streaming pass over `char_indices` — O(chunks) memory instead of
        // O(chars).
        let mut starts: Vec<usize> = (0..total_chars).step_by(step).collect();
        if starts.is_empty() {
            starts.push(0);
        }
        // Char positions whose byte offsets we need, in ascending order.
        let mut needed: Vec<usize> = Vec::with_capacity(starts.len() * 2);
        for &s in &starts {
            needed.push(s);
            needed.push((s + chunk_size).min(total_chars));
        }
        needed.sort_unstable();
        needed.dedup();

        let mut byte_at: Vec<usize> = vec![0; needed.len()];
        {
            let mut ni = 0usize;
            let mut char_idx = 0usize;
            // Position 0 always maps to byte 0 (already set).
            while ni < needed.len() && needed[ni] == 0 {
                ni += 1;
            }
            for (byte_off, _) in text.char_indices() {
                char_idx += 1;
                while ni < needed.len() && needed[ni] == char_idx {
                    byte_at[ni] = byte_off;
                    ni += 1;
                }
                if ni >= needed.len() {
                    break;
                }
            }
            // The terminal position (total_chars) maps to text.len().
            while ni < needed.len() {
                byte_at[ni] = text.len();
                ni += 1;
            }
        }
        let byte_of = |pos: usize| -> usize {
            match needed.binary_search(&pos) {
                Ok(i) => byte_at[i],
                Err(_) => text.len(),
            }
        };

        let mut chunk_idx = 0;
        for &start_char in &starts {
            let end_char = (start_char + chunk_size).min(total_chars);
            let start_byte = byte_of(start_char);
            let end_byte = byte_of(end_char);

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
