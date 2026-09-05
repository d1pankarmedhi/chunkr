use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
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

/// Process-wide cache of fully-built BPE rank tables, one per encoding.
///
/// Building a `CoreBPE` from embedded data costs ~86ms, so every
/// `TokenChunker` construction shares these instead of rebuilding them.
/// `CoreBPE` is `Send + Sync` (and already held behind `Arc` in
/// `TokenChunker`), so handing out cloned `Arc`s is safe.
static CL100K_BPE: OnceLock<Arc<CoreBPE>> = OnceLock::new();
static O200K_BPE: OnceLock<Arc<CoreBPE>> = OnceLock::new();
static P50K_BPE: OnceLock<Arc<CoreBPE>> = OnceLock::new();
static R50K_BPE: OnceLock<Arc<CoreBPE>> = OnceLock::new();

/// Return a shared handle to the cached BPE tables for `encoding`.
fn shared_bpe(encoding: TokenEncoding) -> Result<Arc<CoreBPE>, ChunkrError> {
    let cell = match encoding {
        TokenEncoding::Cl100kBase => &CL100K_BPE,
        TokenEncoding::O200kBase => &O200K_BPE,
        TokenEncoding::P50kBase => &P50K_BPE,
        TokenEncoding::R50kBase => &R50K_BPE,
    };
    // `get_or_init` cannot return `Result`, so a failed build panics here.
    // The embedded tables are compile-time data and must always parse.
    let bpe = cell.get_or_init(|| {
        Arc::new(
            encoding
                .get_bpe()
                .expect("embedded BPE valid"),
        )
    });
    Ok(Arc::clone(bpe))
}

/// Splits text into chunks by token count using fast BPE tokenizers.
pub struct TokenChunker {
    pub chunk_size: usize,
    pub overlap: usize,
    pub encoding: TokenEncoding,
    bpe: Arc<CoreBPE>,
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
        Self {
            chunk_size: self.chunk_size,
            overlap: self.overlap,
            encoding: self.encoding,
            // Share the (expensive to construct) BPE rank tables instead of
            // rebuilding them from embedded data on every clone.
            bpe: Arc::clone(&self.bpe),
        }
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
        let bpe = shared_bpe(encoding)?;
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
        // Re-validate here: `chunk_size` / `overlap` are public fields, so a
        // caller may have mutated them after construction. Without this guard
        // `self.chunk_size - self.overlap` would underflow and panic (or wrap
        // to `usize::MAX` in release, hanging/OOMing the loop below).
        if self.chunk_size == 0 {
            return Err(ChunkrError::InvalidChunkSize(0));
        }
        if self.overlap >= self.chunk_size {
            return Err(ChunkrError::InvalidOverlap {
                chunk_size: self.chunk_size,
                overlap: self.overlap,
            });
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
                .decode(token_slice)
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
        // Share the BPE tables via Arc instead of rebuilding them.
        if chunk_size == 0 {
            return Err(ChunkrError::InvalidChunkSize(0).to_string());
        }
        if overlap >= chunk_size {
            return Err(ChunkrError::InvalidOverlap { chunk_size, overlap }.to_string());
        }
        let chunker = Self {
            chunk_size,
            overlap,
            encoding: self.encoding,
            bpe: Arc::clone(&self.bpe),
        };
        chunker.chunk(text).map_err(|e| e.to_string())
    }
}
