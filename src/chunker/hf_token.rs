use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use serde_json::Value;
use tokenizers::Tokenizer;

use crate::chunker::base::{BaseChunker, Chunker};
use crate::error::ChunkrError;
use crate::structures::document::Document;

/// Splits text into chunks by token count using any Hugging Face tokenizer
/// (e.g. Llama 3, Mistral, Gemma, Qwen, BERT, BGE, or custom tokenizer.json).
#[derive(Clone)]
pub struct HFTokenChunker {
    pub chunk_size: usize,
    pub overlap: usize,
    tokenizer: Arc<Tokenizer>,
}

impl std::fmt::Debug for HFTokenChunker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HFTokenChunker")
            .field("chunk_size", &self.chunk_size)
            .field("overlap", &self.overlap)
            .finish()
    }
}

impl HFTokenChunker {
    /// Load from a tokenizer.json file located on disk
    pub fn from_file<P: AsRef<Path>>(
        path: P,
        chunk_size: usize,
        overlap: usize,
    ) -> Result<Self, ChunkrError> {
        if overlap >= chunk_size {
            return Err(ChunkrError::InvalidOverlap {
                chunk_size,
                overlap,
            });
        }
        let tokenizer = Tokenizer::from_file(path)
            .map_err(|e| ChunkrError::TokenizerError(e.to_string()))?;
        Ok(Self {
            chunk_size,
            overlap,
            tokenizer: Arc::new(tokenizer),
        })
    }

    /// Load from a tokenizer JSON string representation
    pub fn from_json(
        json_str: &str,
        chunk_size: usize,
        overlap: usize,
    ) -> Result<Self, ChunkrError> {
        if overlap >= chunk_size {
            return Err(ChunkrError::InvalidOverlap {
                chunk_size,
                overlap,
            });
        }
        let tokenizer = Tokenizer::from_bytes(json_str.as_bytes())
            .map_err(|e| ChunkrError::TokenizerError(e.to_string()))?;
        Ok(Self {
            chunk_size,
            overlap,
            tokenizer: Arc::new(tokenizer),
        })
    }

    /// Construct from an existing tokenizers::Tokenizer instance
    pub fn from_tokenizer(
        tokenizer: Tokenizer,
        chunk_size: usize,
        overlap: usize,
    ) -> Result<Self, ChunkrError> {
        if overlap >= chunk_size {
            return Err(ChunkrError::InvalidOverlap {
                chunk_size,
                overlap,
            });
        }
        Ok(Self {
            chunk_size,
            overlap,
            tokenizer: Arc::new(tokenizer),
        })
    }

    /// Count tokens in text using the Hugging Face tokenizer
    pub fn count_tokens(&self, text: &str) -> Result<usize, ChunkrError> {
        let encoding = self
            .tokenizer
            .encode(text, false)
            .map_err(|e| ChunkrError::TokenizerError(e.to_string()))?;
        Ok(encoding.get_ids().len())
    }

    /// Access the underlying tokenizers::Tokenizer
    pub fn tokenizer(&self) -> &Tokenizer {
        &self.tokenizer
    }
}

impl Chunker for HFTokenChunker {
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError> {
        if text.trim().is_empty() {
            return Err(ChunkrError::EmptyInput);
        }
        // `chunk_size` / `overlap` are public and mutable after construction.
        if self.chunk_size == 0 {
            return Err(ChunkrError::InvalidChunkSize(0));
        }
        if self.overlap >= self.chunk_size {
            return Err(ChunkrError::InvalidOverlap {
                chunk_size: self.chunk_size,
                overlap: self.overlap,
            });
        }

        let encoding = self
            .tokenizer
            .encode(text, false)
            .map_err(|e| ChunkrError::TokenizerError(e.to_string()))?;

        let ids = encoding.get_ids();
        let total_tokens = ids.len();

        if total_tokens == 0 {
            return Err(ChunkrError::EmptyInput);
        }

        let step = (self.chunk_size.saturating_sub(self.overlap)).max(1);
        let mut result = Vec::with_capacity((total_tokens / step) + 1);
        let mut start_token = 0;
        let mut chunk_idx = 0;

        while start_token < total_tokens {
            let end_token = (start_token + self.chunk_size).min(total_tokens);
            let token_slice = &ids[start_token..end_token];

            let decoded = self
                .tokenizer
                .decode(token_slice, true)
                .map_err(|e| ChunkrError::TokenizerError(e.to_string()))?;

            let trimmed = decoded.trim();
            if !trimmed.is_empty() {
                let mut metadata = HashMap::with_capacity(6);
                metadata.insert("token_count".to_string(), Value::from(token_slice.len()));
                metadata.insert("token_start".to_string(), Value::from(start_token));
                metadata.insert("token_end".to_string(), Value::from(end_token));
                metadata.insert("length".to_string(), Value::from(trimmed.len()));
                metadata.insert("chunk_index".to_string(), Value::from(chunk_idx));
                metadata.insert("tokenizer".to_string(), Value::from("huggingface"));

                result.push(Document {
                    content: trimmed.to_string(),
                    metadata,
                });
                chunk_idx += 1;
            }

            if end_token >= total_tokens {
                break;
            }
            start_token += step;
        }

        Ok(result)
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for HFTokenChunker {
    fn chunk_text(
        &self,
        text: &str,
        chunk_size: usize,
        overlap: usize,
    ) -> Result<Vec<Document>, String> {
        if chunk_size == 0 {
            return Err(ChunkrError::InvalidChunkSize(0).to_string());
        }
        if overlap >= chunk_size {
            return Err(ChunkrError::InvalidOverlap { chunk_size, overlap }.to_string());
        }
        let mut cloned = self.clone();
        cloned.chunk_size = chunk_size;
        cloned.overlap = overlap;
        cloned.chunk(text).map_err(|e| e.to_string())
    }
}
