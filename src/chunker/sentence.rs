use std::collections::HashMap;
use serde_json::Value;

use crate::chunker::base::{BaseChunker, Chunker};
use crate::error::ChunkrError;
use crate::structures::document::Document;

/// Known common abbreviations that should NOT trigger sentence boundaries
const KNOWN_ABBREVIATIONS: &[&str] = &[
    "mr.", "mrs.", "ms.", "dr.", "prof.", "sr.", "jr.", "vs.", "etc.", "e.g.",
    "i.e.", "inc.", "corp.", "co.", "ltd.", "u.s.", "u.k.", "u.s.a.", "p.m.", "a.m.",
    "jan.", "feb.", "mar.", "apr.", "jun.", "jul.", "aug.", "sep.", "sept.", "oct.",
    "nov.", "dec.", "dept.", "approx.", "est.", "fig.", "al.", "no.", "vol.", "pp.",
];

/// Splits text into chunks by sentences while preserving abbreviations, decimals, and quotes.
#[derive(Debug, Clone)]
pub struct SentenceChunker {
    pub sentences_per_chunk: usize,
    pub sentence_overlap: usize,
    pub max_characters: Option<usize>,
}

impl SentenceChunker {
    /// Create a SentenceChunker with default: 3 sentences per chunk, 1 sentence overlap
    pub fn new() -> Self {
        Self {
            sentences_per_chunk: 3,
            sentence_overlap: 1,
            max_characters: None,
        }
    }

    /// Builder for sentences per chunk
    pub fn with_sentences_per_chunk(mut self, count: usize) -> Self {
        self.sentences_per_chunk = count;
        self
    }

    /// Builder for sentence overlap
    pub fn with_sentence_overlap(mut self, overlap: usize) -> Self {
        self.sentence_overlap = overlap;
        self
    }

    /// Builder for optional maximum character cutoff per chunk
    pub fn with_max_characters(mut self, max_chars: usize) -> Self {
        self.max_characters = Some(max_chars);
        self
    }

    /// High-precision sentence boundary detector
    pub fn split_sentences<'a>(text: &'a str) -> Vec<&'a str> {
        let mut sentences = Vec::new();
        let bytes = text.as_bytes();
        let len = bytes.len();
        let mut start = 0;
        let mut i = 0;

        while i < len {
            let b = bytes[i];

            if b == b'.' || b == b'!' || b == b'?' {
                let is_end = if i + 1 >= len {
                    true
                } else {
                    let next_byte = bytes[i + 1];
                    // Followed by whitespace, newline, or quote + whitespace
                    if next_byte == b' ' || next_byte == b'\n' || next_byte == b'\r' || next_byte == b'\t' {
                        true
                    } else if (next_byte == b'"' || next_byte == b'\'' || next_byte == b')' || next_byte == b']')
                        && (i + 2 >= len || bytes[i + 2] == b' ' || bytes[i + 2] == b'\n' || bytes[i + 2] == b'\r')
                    {
                        true
                    } else {
                        false
                    }
                };

                if is_end {
                    // Check if period is part of decimal number (e.g. 3.14)
                    if b == b'.' && i > 0 && i + 1 < len {
                        if bytes[i - 1].is_ascii_digit() && bytes[i + 1].is_ascii_digit() {
                            i += 1;
                            continue;
                        }
                    }

                    // Check if period is part of ellipsis (...)
                    if b == b'.' && (i + 1 < len && bytes[i + 1] == b'.' || (i > 0 && bytes[i - 1] == b'.')) {
                        i += 1;
                        continue;
                    }

                    // Check if preceding token is an abbreviation (e.g. "Dr.", "e.g.")
                    if b == b'.' {
                        let prefix = &text[start..=i];
                        let last_word = prefix
                            .split_whitespace()
                            .last()
                            .unwrap_or("")
                            .to_lowercase();

                        if KNOWN_ABBREVIATIONS.contains(&last_word.as_str()) {
                            i += 1;
                            continue;
                        }
                    }

                    // Determine split end position including trailing quote if present
                    let mut split_end = i + 1;
                    if split_end < len && (bytes[split_end] == b'"' || bytes[split_end] == b'\'' || bytes[split_end] == b')' || bytes[split_end] == b']') {
                        split_end += 1;
                    }

                    let sentence = text[start..split_end].trim();
                    if !sentence.is_empty() {
                        sentences.push(sentence);
                    }
                    start = split_end;
                    i = split_end;
                    continue;
                }
            }
            i += 1;
        }

        if start < len {
            let tail = text[start..].trim();
            if !tail.is_empty() {
                sentences.push(tail);
            }
        }

        sentences
    }
}

impl Default for SentenceChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunker for SentenceChunker {
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError> {
        if text.trim().is_empty() {
            return Err(ChunkrError::EmptyInput);
        }
        if self.sentences_per_chunk == 0 {
            return Err(ChunkrError::InvalidChunkSize(0));
        }
        if self.sentence_overlap >= self.sentences_per_chunk {
            return Err(ChunkrError::InvalidOverlap {
                chunk_size: self.sentences_per_chunk,
                overlap: self.sentence_overlap,
            });
        }

        let sentences = Self::split_sentences(text);
        let total_sentences = sentences.len();

        if total_sentences == 0 {
            return Err(ChunkrError::EmptyInput);
        }

        let mut result = Vec::new();
        let step = self.sentences_per_chunk - self.sentence_overlap;
        let mut start_idx = 0;
        let mut chunk_idx = 0;

        while start_idx < total_sentences {
            let end_idx = (start_idx + self.sentences_per_chunk).min(total_sentences);
            let chunk_sentences = &sentences[start_idx..end_idx];
            let joined_text = chunk_sentences.join(" ");

            let content = match self.max_characters {
                Some(max) => {
                    if let Some((idx, _)) = joined_text.char_indices().nth(max) {
                        joined_text[..idx].trim().to_string()
                    } else {
                        joined_text
                    }
                }
                None => joined_text,
            };

            let mut metadata = HashMap::with_capacity(4);
            metadata.insert("length".to_string(), Value::from(content.len()));
            metadata.insert("sentence_count".to_string(), Value::from(end_idx - start_idx));
            metadata.insert("start_sentence".to_string(), Value::from(start_idx));
            metadata.insert("end_sentence".to_string(), Value::from(end_idx));
            metadata.insert("chunk_index".to_string(), Value::from(chunk_idx));

            result.push(Document {
                content,
                metadata,
            });
            chunk_idx += 1;

            if end_idx == total_sentences {
                break;
            }

            start_idx += step;
        }

        Ok(result)
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for SentenceChunker {
    fn chunk_text(
        &self,
        text: &str,
        chunk_size: usize,
        overlap: usize,
    ) -> Result<Vec<Document>, String> {
        let chunker = self
            .clone()
            .with_sentences_per_chunk(chunk_size)
            .with_sentence_overlap(overlap);
        chunker.chunk(text).map_err(|e| e.to_string())
    }
}

/// Splits text into chunks by paragraphs.
#[derive(Debug, Clone)]
pub struct ParagraphChunker {
    pub paragraphs_per_chunk: usize,
    pub paragraph_overlap: usize,
}

impl ParagraphChunker {
    pub fn new() -> Self {
        Self {
            paragraphs_per_chunk: 2,
            paragraph_overlap: 0,
        }
    }

    pub fn with_paragraphs_per_chunk(mut self, count: usize) -> Self {
        self.paragraphs_per_chunk = count;
        self
    }

    pub fn with_paragraph_overlap(mut self, overlap: usize) -> Self {
        self.paragraph_overlap = overlap;
        self
    }

    pub fn split_paragraphs<'a>(text: &'a str) -> Vec<&'a str> {
        text.split("\n\n")
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .collect()
    }
}

impl Default for ParagraphChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunker for ParagraphChunker {
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError> {
        if text.trim().is_empty() {
            return Err(ChunkrError::EmptyInput);
        }
        if self.paragraphs_per_chunk == 0 {
            return Err(ChunkrError::InvalidChunkSize(0));
        }
        if self.paragraph_overlap >= self.paragraphs_per_chunk {
            return Err(ChunkrError::InvalidOverlap {
                chunk_size: self.paragraphs_per_chunk,
                overlap: self.paragraph_overlap,
            });
        }

        let paragraphs = Self::split_paragraphs(text);
        let total_paragraphs = paragraphs.len();

        if total_paragraphs == 0 {
            return Err(ChunkrError::EmptyInput);
        }

        let mut result = Vec::new();
        let step = self.paragraphs_per_chunk - self.paragraph_overlap;
        let mut start_idx = 0;
        let mut chunk_idx = 0;

        while start_idx < total_paragraphs {
            let end_idx = (start_idx + self.paragraphs_per_chunk).min(total_paragraphs);
            let chunk_paragraphs = &paragraphs[start_idx..end_idx];
            let content = chunk_paragraphs.join("\n\n");

            let mut metadata = HashMap::with_capacity(4);
            metadata.insert("length".to_string(), Value::from(content.len()));
            metadata.insert("paragraph_count".to_string(), Value::from(end_idx - start_idx));
            metadata.insert("start_paragraph".to_string(), Value::from(start_idx));
            metadata.insert("end_paragraph".to_string(), Value::from(end_idx));
            metadata.insert("chunk_index".to_string(), Value::from(chunk_idx));

            result.push(Document {
                content,
                metadata,
            });
            chunk_idx += 1;

            if end_idx == total_paragraphs {
                break;
            }

            start_idx += step;
        }

        Ok(result)
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for ParagraphChunker {
    fn chunk_text(
        &self,
        text: &str,
        chunk_size: usize,
        overlap: usize,
    ) -> Result<Vec<Document>, String> {
        let chunker = self
            .clone()
            .with_paragraphs_per_chunk(chunk_size)
            .with_paragraph_overlap(overlap);
        chunker.chunk(text).map_err(|e| e.to_string())
    }
}
