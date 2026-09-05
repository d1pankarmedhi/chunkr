use memchr::memmem;
use serde_json::Value;
use std::collections::HashMap;

use crate::chunker::base::{BaseChunker, Chunker};
use crate::error::ChunkrError;
use crate::structures::document::Document;

/// Defines where the separator is kept when splitting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeepSeparator {
    #[default]
    Start,
    End,
    None,
}

/// Recursively splits text into chunks using a hierarchical list of separators.
///
/// Tries separators in order (e.g. paragraphs "\n\n", newlines "\n", spaces " ", characters "").
/// Preserves semantic structure while ensuring chunks fit within `chunk_size` and overlap by `overlap`.
#[derive(Debug, Clone)]
pub struct RecursiveChunker {
    pub chunk_size: usize,
    pub overlap: usize,
    pub separators: Vec<String>,
    pub keep_separator: KeepSeparator,
}

impl RecursiveChunker {
    /// Create a new RecursiveChunker with default separators `["\n\n", "\n", " ", ""]`,
    /// chunk_size 1000, and overlap 200.
    pub fn new() -> Self {
        Self {
            chunk_size: 1000,
            overlap: 200,
            separators: vec![
                "\n\n".to_string(),
                "\n".to_string(),
                " ".to_string(),
                "".to_string(),
            ],
            keep_separator: KeepSeparator::Start,
        }
    }

    /// Builder method for chunk size
    pub fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = chunk_size;
        self
    }

    /// Builder method for overlap
    pub fn with_overlap(mut self, overlap: usize) -> Self {
        self.overlap = overlap;
        self
    }

    /// Builder method for custom separators hierarchy
    pub fn with_separators(mut self, separators: Vec<String>) -> Self {
        self.separators = separators;
        self
    }

    /// Builder method for separator retention behavior
    pub fn with_keep_separator(mut self, keep_separator: KeepSeparator) -> Self {
        self.keep_separator = keep_separator;
        self
    }

    /// Recursively split text given a list of separator candidates directly into a buffer
    fn split_recursive_into<'a>(
        &self,
        text: &'a str,
        separators: &[String],
        out: &mut Vec<&'a str>,
    ) {
        if text.len() <= self.chunk_size {
            out.push(text);
            return;
        }

        // Find the first matching separator in the priority list.
        // NOTE: a single `Finder::find` probe per candidate avoids the extra
        // full-text scan that `str::contains` + `find_iter` would perform.
        let mut chosen_sep = None;
        let mut remaining_seps = &[][..];

        for (i, sep) in separators.iter().enumerate() {
            if sep.is_empty() {
                chosen_sep = Some(sep.as_str());
                remaining_seps = &[];
                break;
            }
            let finder = memmem::Finder::new(sep.as_bytes());
            if finder.find(text.as_bytes()).is_some() {
                chosen_sep = Some(sep.as_str());
                remaining_seps = &separators[i + 1..];
                break;
            }
        }

        let sep = match chosen_sep {
            Some(s) => s,
            None => {
                self.split_by_char_indices_into(text, out);
                return;
            }
        };

        let splits = self.split_by_separator(text, sep);

        for s in splits {
            if s.len() <= self.chunk_size {
                out.push(s);
            } else if !remaining_seps.is_empty() {
                self.split_recursive_into(s, remaining_seps, out);
            } else {
                self.split_by_char_indices_into(s, out);
            }
        }
    }

    /// Split text by single separator respecting keep_separator mode using SIMD search
    fn split_by_separator<'a>(&self, text: &'a str, sep: &str) -> Vec<&'a str> {
        if sep.is_empty() {
            let mut out = Vec::new();
            self.split_by_char_indices_into(text, &mut out);
            return out;
        }

        let mut result = Vec::new();
        let mut start = 0;
        let finder = memmem::Finder::new(sep.as_bytes());

        for idx in finder.find_iter(text.as_bytes()) {
            match self.keep_separator {
                KeepSeparator::None => {
                    let part = &text[start..idx];
                    if !part.is_empty() {
                        result.push(part);
                    }
                    start = idx + sep.len();
                }
                KeepSeparator::Start => {
                    if idx > start {
                        let part = &text[start..idx];
                        result.push(part);
                    }
                    start = idx;
                }
                KeepSeparator::End => {
                    let end = idx + sep.len();
                    let part = &text[start..end];
                    result.push(part);
                    start = end;
                }
            }
        }

        if start < text.len() {
            let part = &text[start..];
            if !part.is_empty() {
                result.push(part);
            }
        }

        result
    }

    /// Fallback char-boundary splitter.
    ///
    /// Streams over `char_indices` without collecting the whole index table
    /// first (the old version allocated a `Vec` of one entry per char — i.e.
    /// ~16 bytes/char of temporary memory on large inputs).
    fn split_by_char_indices_into<'a>(&self, text: &'a str, out: &mut Vec<&'a str>) {
        if text.is_empty() {
            return;
        }
        // Byte offsets at which each chunk starts.
        let mut chunk_starts: Vec<usize> = Vec::new();
        chunk_starts.push(0);
        let mut chars_in_chunk = 0usize;

        for (byte_idx, _) in text.char_indices() {
            chars_in_chunk += 1;
            if chars_in_chunk == self.chunk_size {
                // Next chunk starts at the following char boundary.
                let next_start = text[byte_idx..]
                    .char_indices()
                    .nth(1)
                    .map(|(off, _)| byte_idx + off)
                    .unwrap_or(text.len());
                chunk_starts.push(next_start);
                chars_in_chunk = 0;
            }
        }

        if chunk_starts.len() == 1 {
            out.push(text);
            return;
        }

        for (i, &start) in chunk_starts.iter().enumerate() {
            let end = if i + 1 < chunk_starts.len() {
                chunk_starts[i + 1]
            } else {
                text.len()
            };
            if start < end {
                out.push(&text[start..end]);
            }
        }
    }

    /// High performance merge with sliding window O(N)
    fn merge_splits(&self, splits: &[&str]) -> Vec<String> {
        let n = splits.len();
        if n == 0 {
            return Vec::new();
        }

        let mut docs = Vec::with_capacity((n / 4) + 1);
        let piece_lens: Vec<usize> = splits.iter().map(|p| p.len()).collect();
        let mut start_idx = 0;
        let mut current_len = 0;

        for i in 0..n {
            let p_len = piece_lens[i];

            if current_len + p_len > self.chunk_size && i > start_idx {
                let mut joined = String::with_capacity(current_len);
                for j in start_idx..i {
                    joined.push_str(splits[j]);
                }
                let trimmed = joined.trim();
                if !trimmed.is_empty() {
                    docs.push(trimmed.to_string());
                }

                let old_start = start_idx;
                while start_idx < i && current_len > self.overlap {
                    current_len -= piece_lens[start_idx];
                    start_idx += 1;
                }
                if start_idx == old_start && start_idx < i {
                    current_len = current_len.saturating_sub(piece_lens[start_idx]);
                    start_idx += 1;
                }
            }

            current_len += p_len;
        }

        if start_idx < n {
            let mut joined = String::with_capacity(current_len);
            for j in start_idx..n {
                joined.push_str(splits[j]);
            }
            let trimmed = joined.trim();
            if !trimmed.is_empty() {
                docs.push(trimmed.to_string());
            }
        }

        docs
    }
}

impl Default for RecursiveChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunker for RecursiveChunker {
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError> {
        if text.trim().is_empty() {
            return Err(ChunkrError::EmptyInput);
        }
        if self.chunk_size == 0 {
            return Err(ChunkrError::InvalidChunkSize(0));
        }
        if self.overlap >= self.chunk_size {
            return Err(ChunkrError::InvalidOverlap {
                chunk_size: self.chunk_size,
                overlap: self.overlap,
            });
        }

        let mut raw_splits = Vec::new();
        self.split_recursive_into(text, &self.separators, &mut raw_splits);
        let merged_chunks = self.merge_splits(&raw_splits);

        let mut result = Vec::with_capacity(merged_chunks.len());
        for (chunk_idx, content) in merged_chunks.into_iter().enumerate() {
            let mut metadata = HashMap::with_capacity(2);
            metadata.insert("length".to_string(), Value::from(content.len()));
            metadata.insert("chunk_index".to_string(), Value::from(chunk_idx));

            result.push(Document { content, metadata });
        }

        Ok(result)
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for RecursiveChunker {
    fn chunk_text(
        &self,
        text: &str,
        chunk_size: usize,
        overlap: usize,
    ) -> Result<Vec<Document>, String> {
        let chunker = self
            .clone()
            .with_chunk_size(chunk_size)
            .with_overlap(overlap);
        chunker.chunk(text).map_err(|e| e.to_string())
    }
}
