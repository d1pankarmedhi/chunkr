use std::collections::HashMap;
use std::sync::Arc;
use serde_json::Value;

use crate::chunker::base::{BaseChunker, Chunker};
use crate::chunker::sentence::SentenceChunker;
use crate::error::ChunkrError;
use crate::structures::document::Document;

/// Interface for extracting atomic factual propositions from text
pub trait PropositionExtractor: Send + Sync {
    fn extract_propositions(&self, sentence: &str) -> Result<Vec<String>, ChunkrError>;
}

/// Rule-based syntactic proposition extractor with subject resolution across clauses
#[derive(Debug, Clone, Default)]
pub struct SyntacticPropositionExtractor;

impl SyntacticPropositionExtractor {
    pub fn new() -> Self {
        Self
    }

    /// Extract grammatical subject from the beginning of a clause/sentence
    fn extract_subject(sentence: &str) -> Option<String> {
        let trimmed = sentence.trim();
        let words: Vec<&str> = trimmed.split_whitespace().collect();
        if words.is_empty() {
            return None;
        }

        // Search for common verbs (is, was, are, were, has, had, constructed, located, contains, etc.)
        let verb_markers = [
            "is", "was", "are", "were", "has", "have", "had", "can", "will",
            "built", "located", "designed", "created", "published", "released",
            "serves", "provides", "features", "includes", "supports", "uses",
        ];

        let mut verb_idx = None;
        for (i, word) in words.iter().enumerate() {
            let clean = word.trim_matches(|c: char| !c.is_alphabetic()).to_lowercase();
            if verb_markers.contains(&clean.as_str()) && i > 0 {
                verb_idx = Some(i);
                break;
            }
        }

        if let Some(idx) = verb_idx {
            let subject = words[..idx].join(" ");
            Some(subject)
        } else if words.len() >= 2 {
            Some(words[..2].join(" "))
        } else {
            None
        }
    }

    /// Decompose complex sentence into atomic propositions
    pub fn decompose_sentence(sentence: &str) -> Vec<String> {
        let trimmed = sentence.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }

        let subject = Self::extract_subject(trimmed);
        let mut propositions = Vec::new();

        // 1. Check relative clause: ", which ", ", who ", ", where ", ", that "
        let rel_markers = [", which ", ", who ", ", where ", ", that "];
        let base_text = trimmed.to_string();

        for marker in &rel_markers {
            if let Some(pos) = base_text.find(marker) {
                let prefix = base_text[..pos].trim().trim_end_matches(',');
                let after_marker = &base_text[pos + marker.len()..];

                let (rel_body, rest_of_sentence) = match after_marker.find(", ") {
                    Some(comma_pos) => (
                        after_marker[..comma_pos].trim(),
                        Some(after_marker[comma_pos + 2..].trim()),
                    ),
                    None => (after_marker.trim().trim_end_matches('.'), None),
                };

                let subj = prefix;

                if let Some(rest) = rest_of_sentence {
                    let main_sentence = format!("{} {}", prefix, rest.trim_end_matches('.'));
                    propositions.push(format!("{}.", main_sentence));
                    propositions.push(format!("{} {}.", subj, rel_body.trim_end_matches('.')));
                } else {
                    propositions.push(format!("{}.", prefix));
                    let antecedent = prefix.split_whitespace().last().unwrap_or(subj);
                    let clean_rel = rel_body.trim_end_matches('.');
                    let starts_with_verb = clean_rel
                        .split_whitespace()
                        .next()
                        .map(|w| {
                            let clean = w.to_lowercase();
                            ["is", "was", "are", "were", "has", "have", "had", "built", "located", "created", "features", "serves", "supports"].contains(&clean.as_str())
                        })
                        .unwrap_or(false);
                    if starts_with_verb {
                        propositions.push(format!("{} {}.", antecedent, clean_rel));
                    } else {
                        propositions.push(format!("{}.", clean_rel));
                    }
                }
                return propositions;
            }
        }

        // 2. Check coordinating conjunction clauses: ", and ", ", but ", "; ", ", while "
        let coord_markers = ["; ", ", and ", ", but ", ", however, ", ", whereas ", ", while "];
        for marker in &coord_markers {
            if base_text.contains(marker) {
                let parts: Vec<&str> = base_text.split(marker).collect();
                for (i, part) in parts.iter().enumerate() {
                    let clean_part = part.trim().trim_matches(|c: char| c == '.' || c == ',');
                    if clean_part.is_empty() {
                        continue;
                    }

                    if i > 0 && !clean_part.contains(' ') {
                        continue;
                    }

                    // If second clause starts without subject, prepend inferred subject
                    let starts_with_verb = clean_part
                        .split_whitespace()
                        .next()
                        .map(|w| {
                            let clean = w.to_lowercase();
                            ["is", "was", "are", "were", "has", "have", "had", "welcomes", "provides", "features", "serves", "supports"].contains(&clean.as_str())
                        })
                        .unwrap_or(false);

                    if starts_with_verb && subject.is_some() {
                        propositions.push(format!("{} {}.", subject.as_ref().unwrap(), clean_part));
                    } else {
                        propositions.push(format!("{}.", clean_part));
                    }
                }

                if propositions.len() > 1 {
                    return propositions;
                } else {
                    propositions.clear();
                }
            }
        }

        // If no clause splitting matched, return original sentence
        vec![trimmed.to_string()]
    }
}

impl PropositionExtractor for SyntacticPropositionExtractor {
    fn extract_propositions(&self, sentence: &str) -> Result<Vec<String>, ChunkrError> {
        Ok(Self::decompose_sentence(sentence))
    }
}

/// Custom proposition extractor using a closure (e.g. for LLM prompt extractors)
pub struct CustomPropositionExtractor<F>
where
    F: Fn(&str) -> Result<Vec<String>, ChunkrError> + Send + Sync,
{
    func: F,
}

impl<F> CustomPropositionExtractor<F>
where
    F: Fn(&str) -> Result<Vec<String>, ChunkrError> + Send + Sync,
{
    pub fn new(func: F) -> Self {
        Self { func }
    }
}

impl<F> PropositionExtractor for CustomPropositionExtractor<F>
where
    F: Fn(&str) -> Result<Vec<String>, ChunkrError> + Send + Sync,
{
    fn extract_propositions(&self, sentence: &str) -> Result<Vec<String>, ChunkrError> {
        (self.func)(sentence)
    }
}

/// Proposition-based chunker that breaks text down into atomic, self-contained factual claims
pub struct PropositionChunker {
    pub extractor: Arc<dyn PropositionExtractor>,
    pub propositions_per_chunk: usize,
    pub proposition_overlap: usize,
}

impl std::fmt::Debug for PropositionChunker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PropositionChunker")
            .field("propositions_per_chunk", &self.propositions_per_chunk)
            .field("proposition_overlap", &self.proposition_overlap)
            .finish()
    }
}

impl Clone for PropositionChunker {
    fn clone(&self) -> Self {
        Self {
            extractor: Arc::clone(&self.extractor),
            propositions_per_chunk: self.propositions_per_chunk,
            proposition_overlap: self.proposition_overlap,
        }
    }
}

impl PropositionChunker {
    /// Create a PropositionChunker with default syntactic extractor and 1 proposition per chunk
    pub fn new() -> Self {
        Self {
            extractor: Arc::new(SyntacticPropositionExtractor::new()),
            propositions_per_chunk: 1,
            proposition_overlap: 0,
        }
    }

    /// Set custom proposition extractor
    pub fn with_extractor(mut self, extractor: impl PropositionExtractor + 'static) -> Self {
        self.extractor = Arc::new(extractor);
        self
    }

    /// Group N propositions per chunk with specified overlap
    pub fn with_grouping(mut self, count: usize, overlap: usize) -> Result<Self, ChunkrError> {
        if count == 0 {
            return Err(ChunkrError::InvalidChunkSize(0));
        }
        if overlap >= count {
            return Err(ChunkrError::InvalidOverlap {
                chunk_size: count,
                overlap,
            });
        }
        self.propositions_per_chunk = count;
        self.proposition_overlap = overlap;
        Ok(self)
    }
}

impl Default for PropositionChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunker for PropositionChunker {
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError> {
        if text.trim().is_empty() {
            return Err(ChunkrError::EmptyInput);
        }
        // Guard against post-construction mutation of the public fields.
        if self.propositions_per_chunk == 0 {
            return Err(ChunkrError::InvalidChunkSize(0));
        }
        if self.proposition_overlap >= self.propositions_per_chunk {
            return Err(ChunkrError::InvalidOverlap {
                chunk_size: self.propositions_per_chunk,
                overlap: self.proposition_overlap,
            });
        }

        let sentences = SentenceChunker::split_sentences(text);
        let mut all_propositions = Vec::new();

        for sentence in sentences {
            let props = self.extractor.extract_propositions(sentence)?;
            all_propositions.extend(props);
        }

        if all_propositions.is_empty() {
            return Err(ChunkrError::EmptyInput);
        }

        let mut result = Vec::new();
        let step = self.propositions_per_chunk - self.proposition_overlap;
        let mut start_idx = 0;
        let mut chunk_idx = 0;
        let total = all_propositions.len();

        while start_idx < total {
            let end_idx = (start_idx + self.propositions_per_chunk).min(total);
            let chunk_props = &all_propositions[start_idx..end_idx];
            let content = chunk_props.join(" ");

            let mut metadata = HashMap::with_capacity(4);
            metadata.insert("length".to_string(), Value::from(content.len()));
            metadata.insert("proposition_count".to_string(), Value::from(end_idx - start_idx));
            metadata.insert("start_prop_idx".to_string(), Value::from(start_idx));
            metadata.insert("end_prop_idx".to_string(), Value::from(end_idx));
            metadata.insert("chunk_index".to_string(), Value::from(chunk_idx));

            result.push(Document {
                content,
                metadata,
            });
            chunk_idx += 1;

            if end_idx == total {
                break;
            }

            start_idx += step;
        }

        Ok(result)
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for PropositionChunker {
    fn chunk_text(
        &self,
        text: &str,
        chunk_size: usize,
        overlap: usize,
    ) -> Result<Vec<Document>, String> {
        let chunker = self
            .clone()
            .with_grouping(chunk_size.max(1), overlap.min(chunk_size.max(1) - 1))
            .map_err(|e| e.to_string())?;
        chunker.chunk(text).map_err(|e| e.to_string())
    }
}
