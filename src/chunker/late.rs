#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;
use serde_json::Value;
use std::sync::Arc;
use tiktoken_rs::CoreBPE;

use crate::chunker::base::{BaseChunker, Chunker};
use crate::chunker::recursive::RecursiveChunker;
use crate::chunker::token::TokenEncoding;
use crate::error::ChunkrError;
use crate::structures::document::Document;

/// A chunked document paired with its exact token span `[token_start, token_end)`.
pub type ChunkSpans = Vec<(Document, (usize, usize))>;

/// High-performance chunker implementing Late Chunking for LLMs and RAG.
///
/// Late Chunking embeds the entire document first via a transformer encoder to maintain
/// bidirectional attention across the whole context, then mean-pools token embeddings
/// over chunk token spans to produce context-rich chunk embeddings.
///
/// See [`ChunkSpans`] for the `(document, token-span)` pairs returned by
/// [`LateChunker::chunk_spans`].
#[derive(Clone)]
pub struct LateChunker {
    pub encoding: TokenEncoding,
    pub base_chunker: Arc<dyn Chunker>,
    pub normalize: bool,
    bpe: Arc<CoreBPE>,
}

impl std::fmt::Debug for LateChunker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LateChunker")
            .field("encoding", &self.encoding)
            .field("normalize", &self.normalize)
            .finish()
    }
}

impl LateChunker {
    /// Create a new LateChunker with default cl100k_base encoding and RecursiveChunker base
    pub fn new() -> Self {
        let encoding = TokenEncoding::Cl100kBase;
        let bpe = encoding.get_bpe().expect("Failed to initialize BPE");
        let base_chunker = Arc::new(
            RecursiveChunker::new()
                .with_chunk_size(500)
                .with_overlap(50),
        );

        Self {
            encoding,
            base_chunker,
            normalize: true,
            bpe: Arc::new(bpe),
        }
    }

    /// Set the tokenizer encoding (e.g. cl100k_base, o200k_base)
    pub fn with_encoding(mut self, encoding: TokenEncoding) -> Result<Self, ChunkrError> {
        let bpe = encoding.get_bpe()?;
        self.encoding = encoding;
        self.bpe = Arc::new(bpe);
        Ok(self)
    }

    /// Set custom base chunker for generating text chunk boundaries
    pub fn with_base_chunker(mut self, chunker: impl Chunker + 'static) -> Self {
        self.base_chunker = Arc::new(chunker);
        self
    }

    /// Enable or disable L2 normalization on pooled embeddings (default: true)
    pub fn with_normalize(mut self, normalize: bool) -> Self {
        self.normalize = normalize;
        self
    }

    /// Build monotonic byte offset array for every token in the text.
    ///
    /// Single-token `decode` calls are cached by token id: natural text
    /// reuses a small working vocabulary, so distinct tokens << total
    /// tokens and repeat decodes (one heap `String` per token) collapse to
    /// one per distinct id.
    fn build_token_offsets(&self, text: &str) -> (Vec<u32>, Vec<usize>) {
        let tokens = self.bpe.encode_ordinary(text);
        let mut offsets = Vec::with_capacity(tokens.len() + 1);
        offsets.push(0);

        let mut current_offset = 0;
        let mut len_cache: std::collections::HashMap<u32, usize> =
            std::collections::HashMap::with_capacity(1024);
        for &t in &tokens {
            let t_len = match len_cache.get(&t) {
                Some(&len) => len,
                None => {
                    let decoded = self.bpe.decode(&[t]).unwrap_or_default();
                    let len = decoded.len();
                    len_cache.insert(t, len);
                    len
                }
            };
            current_offset += t_len;
            offsets.push(current_offset);
        }

        (tokens, offsets)
    }

    /// Split text and compute exact token span indices [token_start, token_end) for each chunk
    pub fn chunk_spans(&self, text: &str) -> Result<ChunkSpans, ChunkrError> {
        if text.trim().is_empty() {
            return Err(ChunkrError::EmptyInput);
        }

        let (tokens, offsets) = self.build_token_offsets(text);
        let total_tokens = tokens.len();

        let base_chunks = self.base_chunker.chunk(text)?;
        if base_chunks.is_empty() {
            return Ok(Vec::new());
        }

        let mut result = Vec::with_capacity(base_chunks.len());
        let mut search_pos = 0;

        for (chunk_idx, mut doc) in base_chunks.into_iter().enumerate() {
            let content = doc.content.trim();
            if content.is_empty() {
                continue;
            }

            // Locate chunk content within text, advancing past the previous
            // match so repeated/overlapped chunks map to the NEXT occurrence
            // instead of all collapsing onto the first one. The cursor snaps
            // forward to a char boundary so the next slice is always valid.
            let advance_past = |pos: usize| -> usize {
                let mut next = pos.saturating_add(1);
                while next < text.len() && !text.is_char_boundary(next) {
                    next += 1;
                }
                next.min(text.len())
            };
            let (char_start, char_end) = match text[search_pos..].find(content) {
                Some(rel_idx) => {
                    let start = search_pos + rel_idx;
                    let end = start + content.len();
                    search_pos = advance_past(start);
                    (start, end)
                }
                None => {
                    // Fallback search from the beginning if out of order
                    match text.find(content) {
                        Some(start) => {
                            search_pos = advance_past(start);
                            (start, start + content.len())
                        }
                        None => (0, text.len()),
                    }
                }
            };

            // Binary search to find token boundaries
            let token_start = offsets.partition_point(|&off| off < char_start);
            let mut token_end = offsets.partition_point(|&off| off <= char_end);

            if token_end <= token_start {
                token_end = (token_start + 1).min(total_tokens);
            }

            let token_count = token_end.saturating_sub(token_start);

            doc.add_metadata("token_start", Value::from(token_start));
            doc.add_metadata("token_end", Value::from(token_end));
            doc.add_metadata("char_start", Value::from(char_start));
            doc.add_metadata("char_end", Value::from(char_end));
            doc.add_metadata("token_count", Value::from(token_count));
            doc.add_metadata("encoding", Value::from(self.encoding.as_str()));
            doc.add_metadata("chunk_index", Value::from(chunk_idx));

            result.push((doc, (token_start, token_end)));
        }

        Ok(result)
    }

    /// Mean-pool token embeddings over a specified token span [start, end)
    pub fn pool_span(
        token_embeddings: &[Vec<f32>],
        start: usize,
        end: usize,
        normalize: bool,
    ) -> Vec<f32> {
        let total_tokens = token_embeddings.len();
        if total_tokens == 0 || start >= total_tokens || start >= end {
            return Vec::new();
        }

        let end_idx = end.min(total_tokens);
        let span_len = (end_idx - start) as f32;
        let dim = token_embeddings[0].len();
        let mut pooled = vec![0.0f32; dim];

        for vec in &token_embeddings[start..end_idx] {
            for (d, val) in vec.iter().enumerate() {
                if d < dim {
                    pooled[d] += val;
                }
            }
        }

        for val in &mut pooled {
            *val /= span_len;
        }

        if normalize {
            let norm_sq: f32 = pooled.iter().map(|x| x * x).sum();
            if norm_sq > 0.0 {
                let norm = norm_sq.sqrt();
                for val in &mut pooled {
                    *val /= norm;
                }
            }
        }

        pooled
    }

    /// Mean-pool full-document token embeddings for all chunks produced by this chunker
    pub fn pool_embeddings(
        &self,
        token_embeddings: &[Vec<f32>],
        docs: &[Document],
    ) -> Vec<Vec<f32>> {
        #[cfg(not(target_arch = "wasm32"))]
        let iter = docs.par_iter();
        #[cfg(target_arch = "wasm32")]
        let iter = docs.iter();

        iter.map(|doc| {
            let start = doc
                .metadata
                .get("token_start")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            let end = doc
                .metadata
                .get("token_end")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            Self::pool_span(token_embeddings, start, end, self.normalize)
        })
        .collect()
    }

    /// Mean-pool full-document token embeddings for an explicit list of (start, end) spans
    pub fn pool_spans(
        &self,
        token_embeddings: &[Vec<f32>],
        spans: &[(usize, usize)],
    ) -> Vec<Vec<f32>> {
        #[cfg(not(target_arch = "wasm32"))]
        let iter = spans.par_iter();
        #[cfg(target_arch = "wasm32")]
        let iter = spans.iter();

        iter.map(|&(start, end)| Self::pool_span(token_embeddings, start, end, self.normalize))
            .collect()
    }
}

impl Default for LateChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunker for LateChunker {
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError> {
        let pairs = self.chunk_spans(text)?;
        Ok(pairs.into_iter().map(|(doc, _)| doc).collect())
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for LateChunker {
    fn chunk_text(
        &self,
        text: &str,
        chunk_size: usize,
        overlap: usize,
    ) -> Result<Vec<Document>, String> {
        let base = RecursiveChunker::new()
            .with_chunk_size(chunk_size)
            .with_overlap(overlap);
        let chunker = self.clone().with_base_chunker(base);
        chunker.chunk(text).map_err(|e| e.to_string())
    }
}
