use std::collections::HashMap;
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;
use serde_json::Value;

use crate::chunker::base::{BaseChunker, Chunker};
use crate::chunker::sentence::SentenceChunker;
use crate::error::ChunkrError;
use crate::structures::document::Document;

/// Breakpoint threshold calculation strategy for semantic boundaries
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BreakpointThreshold {
    /// Distance threshold calculated at the specified percentile (e.g. 90.0 or 95.0)
    Percentile(f32),
    /// Distance threshold calculated as (mean + multiplier * standard_deviation)
    StandardDeviation(f32),
    /// Distance threshold calculated as (Q3 + 1.5 * IQR)
    Interquartile,
    /// Fixed absolute cosine distance threshold (0.0 to 1.0)
    Absolute(f32),
}

impl Default for BreakpointThreshold {
    fn default() -> Self {
        BreakpointThreshold::Percentile(90.0)
    }
}

/// Interface for generating vector embeddings for text segments
pub trait Embedder: Send + Sync {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, ChunkrError>;
}

/// Zero-dependency fast lexical embedder using TF-IDF and character n-gram hashing
#[derive(Debug, Clone, Default)]
pub struct FastLexicalEmbedder {
    pub dim: usize,
}

impl FastLexicalEmbedder {
    pub fn new() -> Self {
        Self { dim: 256 }
    }

    pub fn with_dim(dim: usize) -> Self {
        Self { dim }
    }

    fn embed_one(text: &str, dim: usize) -> Vec<f32> {
        let mut vec = vec![0.0f32; dim];

        // Hash words without allocating a lowercased `String` per word:
        // fold ASCII case + djb2 in a single byte pass, hashing only
        // alphanumeric bytes (drops surrounding punctuation).
        for word in text.split_whitespace() {
            let mut hash: usize = 5381;
            let mut has_content = false;
            for b in word.bytes() {
                let lower = if b.is_ascii_uppercase() { b + 32 } else { b };
                if lower.is_ascii_alphanumeric() {
                    has_content = true;
                    hash = ((hash << 5).wrapping_add(hash)).wrapping_add(lower as usize);
                }
            }
            if !has_content {
                continue;
            }
            let idx = hash % dim;
            vec[idx] += 1.0;
        }

        // L2 normalize
        let norm_sq: f32 = vec.iter().map(|x| x * x).sum();
        if norm_sq > 0.0 {
            let norm = norm_sq.sqrt();
            for x in &mut vec {
                *x /= norm;
            }
        }

        vec
    }
}

impl Embedder for FastLexicalEmbedder {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, ChunkrError> {
        // Parallelize across sentences with Rayon: each embedding is
        // independent, so batch latency scales with core count.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dim = self.dim.max(1);
            return texts
                .par_iter()
                .map(|text| Ok(Self::embed_one(text, dim)))
                .collect();
        }
        #[cfg(target_arch = "wasm32")]
        {
            Ok(texts.iter().map(|t| Self::embed_one(t, self.dim.max(1))).collect())
        }
    }
}

/// Custom embedder adapter allowing closures
pub struct CustomEmbedder<F>
where
    F: Fn(&[&str]) -> Result<Vec<Vec<f32>>, ChunkrError> + Send + Sync,
{
    func: F,
}

impl<F> CustomEmbedder<F>
where
    F: Fn(&[&str]) -> Result<Vec<Vec<f32>>, ChunkrError> + Send + Sync,
{
    pub fn new(func: F) -> Self {
        Self { func }
    }
}

impl<F> Embedder for CustomEmbedder<F>
where
    F: Fn(&[&str]) -> Result<Vec<Vec<f32>>, ChunkrError> + Send + Sync,
{
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, ChunkrError> {
        (self.func)(texts)
    }
}

/// Splits text based on semantic similarity between consecutive sentences
pub struct SemanticChunker {
    pub embedder: Arc<dyn Embedder>,
    pub threshold: BreakpointThreshold,
    pub min_chunk_size: usize,
    pub max_chunk_size: usize,
    pub buffer_size: usize,
}

impl std::fmt::Debug for SemanticChunker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticChunker")
            .field("threshold", &self.threshold)
            .field("min_chunk_size", &self.min_chunk_size)
            .field("max_chunk_size", &self.max_chunk_size)
            .field("buffer_size", &self.buffer_size)
            .finish()
    }
}

impl Clone for SemanticChunker {
    fn clone(&self) -> Self {
        Self {
            embedder: Arc::clone(&self.embedder),
            threshold: self.threshold,
            min_chunk_size: self.min_chunk_size,
            max_chunk_size: self.max_chunk_size,
            buffer_size: self.buffer_size,
        }
    }
}

impl SemanticChunker {
    /// Create a new SemanticChunker with built-in FastLexicalEmbedder and 90th percentile threshold
    pub fn new() -> Self {
        Self {
            embedder: Arc::new(FastLexicalEmbedder::new()),
            threshold: BreakpointThreshold::Percentile(90.0),
            min_chunk_size: 100,
            max_chunk_size: 2000,
            buffer_size: 1,
        }
    }

    /// Use a custom embedder
    pub fn with_embedder(mut self, embedder: impl Embedder + 'static) -> Self {
        self.embedder = Arc::new(embedder);
        self
    }

    /// Set breakpoint threshold strategy
    pub fn with_threshold(mut self, threshold: BreakpointThreshold) -> Self {
        self.threshold = threshold;
        self
    }

    /// Set minimum and maximum chunk sizes (in characters)
    pub fn with_size_bounds(mut self, min_size: usize, max_size: usize) -> Self {
        self.min_chunk_size = min_size;
        self.max_chunk_size = max_size;
        self
    }

    /// Compute cosine distance: 1.0 - cosine_similarity(u, v).
    ///
    /// Uses true cosine normalization (not just `1 - dot`) so custom
    /// [`Embedder`] implementations that return unnormalized vectors still
    /// produce distances in a sane range instead of garbage thresholds.
    fn cosine_distance(u: &[f32], v: &[f32]) -> f32 {
        let mut dot = 0.0f32;
        let mut nu = 0.0f32;
        let mut nv = 0.0f32;
        for (a, b) in u.iter().zip(v.iter()) {
            dot += a * b;
            nu += a * a;
            nv += b * b;
        }
        if nu <= 0.0 || nv <= 0.0 {
            return 1.0;
        }
        (1.0 - dot / (nu.sqrt() * nv.sqrt())).clamp(0.0, 2.0)
    }

    /// Calculate dynamic distance cutoff threshold from list of distances
    fn compute_cutoff_threshold(&self, distances: &[f32]) -> f32 {
        if distances.is_empty() {
            return 0.5;
        }

        match self.threshold {
            BreakpointThreshold::Absolute(val) => val,
            BreakpointThreshold::Percentile(p) => {
                let mut sorted = distances.to_vec();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let idx = ((p / 100.0) * (sorted.len() as f32 - 1.0)).round() as usize;
                sorted[idx.min(sorted.len() - 1)]
            }
            BreakpointThreshold::StandardDeviation(k) => {
                let n = distances.len() as f32;
                let mean: f32 = distances.iter().sum::<f32>() / n;
                let variance: f32 = distances.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / n;
                let std_dev = variance.sqrt();
                mean + k * std_dev
            }
            BreakpointThreshold::Interquartile => {
                let mut sorted = distances.to_vec();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let n = sorted.len();
                let q1 = sorted[n / 4];
                let q3 = sorted[(3 * n) / 4];
                let iqr = q3 - q1;
                q3 + 1.5 * iqr
            }
        }
    }
}

impl Default for SemanticChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunker for SemanticChunker {
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError> {
        if text.trim().is_empty() {
            return Err(ChunkrError::EmptyInput);
        }

        let sentences = SentenceChunker::split_sentences(text);
        if sentences.len() <= 1 {
            let mut metadata = HashMap::with_capacity(2);
            metadata.insert("length".to_string(), Value::from(text.len()));
            metadata.insert("chunk_index".to_string(), Value::from(0));
            return Ok(vec![Document {
                content: text.trim().to_string(),
                metadata,
            }]);
        }

        // Generate embeddings for all sentences
        let embeddings = self.embedder.embed(&sentences)?;

        // Compute distances between consecutive sentences
        let mut distances = Vec::with_capacity(sentences.len() - 1);
        for i in 0..embeddings.len() - 1 {
            let dist = Self::cosine_distance(&embeddings[i], &embeddings[i + 1]);
            distances.push(dist);
        }

        let cutoff = self.compute_cutoff_threshold(&distances);

        // Group sentences into chunks based on breakpoint distances
        let mut result = Vec::new();
        let mut current_chunk: Vec<&str> = Vec::new();
        let mut current_len = 0;
        let mut chunk_idx = 0;

        for (i, &sentence) in sentences.iter().enumerate() {
            let sent_len = sentence.len();
            current_chunk.push(sentence);
            current_len += sent_len + 1;

            let is_last = i == sentences.len() - 1;
            let distance_above_threshold = if i < distances.len() {
                distances[i] > cutoff
            } else {
                false
            };

            let should_split = is_last
                || (distance_above_threshold && current_len >= self.min_chunk_size)
                || (current_len >= self.max_chunk_size);

            if should_split && !current_chunk.is_empty() {
                let content = current_chunk.join(" ");
                let trimmed = content.trim();

                if !trimmed.is_empty() {
                    let mut metadata = HashMap::with_capacity(4);
                    metadata.insert("length".to_string(), Value::from(trimmed.len()));
                    metadata.insert("sentence_count".to_string(), Value::from(current_chunk.len()));
                    metadata.insert("chunk_index".to_string(), Value::from(chunk_idx));
                    metadata.insert("cutoff_threshold".to_string(), Value::from(cutoff as f64));

                    result.push(Document {
                        content: trimmed.to_string(),
                        metadata,
                    });
                    chunk_idx += 1;
                }

                current_chunk.clear();
                current_len = 0;
            }
        }

        Ok(result)
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for SemanticChunker {
    fn chunk_text(
        &self,
        text: &str,
        _chunk_size: usize,
        _overlap: usize,
    ) -> Result<Vec<Document>, String> {
        self.chunk(text).map_err(|e| e.to_string())
    }
}
