use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chunker::base::{BaseChunker, Chunker};
use crate::chunker::sentence::SentenceChunker;
use crate::error::ChunkrError;
use crate::structures::document::Document;

/// Decision produced by an agentic decision maker for the next sentence
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AgenticDecision {
    /// Append sentence to current chunk
    Append,
    /// Split and begin a new chunk under the given topic
    SplitAndStartNew {
        topic_label: String,
        reason: String,
    },
}

/// Interface for autonomous or model-based chunking decisions
pub trait AgenticDecisionMaker: Send + Sync {
    fn decide(
        &self,
        current_chunk_sentences: &[&str],
        next_sentence: &str,
    ) -> Result<AgenticDecision, ChunkrError>;
}

/// Built-in heuristic agent that inspects discourse transitions, entity shifts, and content word overlap
#[derive(Debug, Clone)]
pub struct HeuristicAgenticDecisionMaker {
    pub max_chunk_characters: usize,
    pub min_chunk_characters: usize,
}

/// Stop words excluded from content-word extraction.
///
/// Module-level slice so `extract_content_words` does not rebuild a
/// `HashSet` on every call (once per sentence).
static STOP_WORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "and", "or", "in",
    "on", "at", "to", "for", "with", "of", "by", "from", "as", "it", "this", "that",
];

/// Discourse transition markers as (marker, comma_marker, topic_label).
///
/// Comma-prefixed forms are precomputed literals so `decide` does not
/// allocate a `format!(", {}", marker)` string per marker per sentence.
static TRANSITIONS: &[(&str, &str, &str)] = &[
    ("in conclusion", ", in conclusion", "Conclusion"),
    ("in summary", ", in summary", "Summary"),
    ("on the other hand", ", on the other hand", "Counter-argument"),
    ("in contrast", ", in contrast", "Contrast"),
    ("furthermore", ", furthermore", "Continuation"),
    ("secondly", ", secondly", "Subsequent Point"),
    ("finally", ", finally", "Final Section"),
    ("moving on to", ", moving on to", "Topic Shift"),
    ("meanwhile", ", meanwhile", "Parallel Topic"),
];

impl HeuristicAgenticDecisionMaker {
    pub fn new() -> Self {
        Self {
            max_chunk_characters: 1200,
            min_chunk_characters: 150,
        }
    }

    pub fn with_size_limits(mut self, min_chars: usize, max_chars: usize) -> Self {
        self.min_chunk_characters = min_chars;
        self.max_chunk_characters = max_chars;
        self
    }

    fn extract_content_words(text: &str) -> HashSet<String> {
        text.split_whitespace()
            .map(|w| {
                w.to_lowercase()
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_string()
            })
            .filter(|w| w.len() > 2 && !STOP_WORDS.contains(&w.as_str()))
            .collect()
    }
}

impl Default for HeuristicAgenticDecisionMaker {
    fn default() -> Self {
        Self::new()
    }
}

impl AgenticDecisionMaker for HeuristicAgenticDecisionMaker {
    fn decide(
        &self,
        current_chunk_sentences: &[&str],
        next_sentence: &str,
    ) -> Result<AgenticDecision, ChunkrError> {
        if current_chunk_sentences.is_empty() {
            return Ok(AgenticDecision::Append);
        }

        let current_text = current_chunk_sentences.join(" ");
        let current_len = current_text.len();

        // Hard maximum character limit
        if current_len + next_sentence.len() >= self.max_chunk_characters {
            return Ok(AgenticDecision::SplitAndStartNew {
                topic_label: "Size Boundary".to_string(),
                reason: "Exceeded max character size limit".to_string(),
            });
        }

        // Check if minimum size reached before evaluating semantic transitions
        if current_len < self.min_chunk_characters {
            return Ok(AgenticDecision::Append);
        }

        // 1. Discourse transition markers indicating new topic or conclusion
        let lower_next = next_sentence.to_lowercase();
        for &(marker, comma_marker, label) in TRANSITIONS {
            if lower_next.starts_with(marker) || lower_next.contains(comma_marker) {
                return Ok(AgenticDecision::SplitAndStartNew {
                    topic_label: label.to_string(),
                    reason: format!("Detected discourse transition marker '{}'", marker),
                });
            }
        }

        // 2. Vocabulary Jaccard similarity between current chunk and candidate sentence
        let current_words = Self::extract_content_words(&current_text);
        let next_words = Self::extract_content_words(next_sentence);

        if !current_words.is_empty() && !next_words.is_empty() {
            let intersection: usize = current_words.intersection(&next_words).count();
            let union: usize = current_words.union(&next_words).count();
            let jaccard = intersection as f64 / union as f64;

            // Low lexical overlap when chunk is sufficiently large indicates topic drift
            if jaccard < 0.05 && current_len >= (self.min_chunk_characters * 2) {
                let sample_word = next_words.iter().next().cloned().unwrap_or_default();
                return Ok(AgenticDecision::SplitAndStartNew {
                    topic_label: format!("Topic on {}", sample_word),
                    reason: "Low lexical overlap and topic divergence detected".to_string(),
                });
            }
        }

        Ok(AgenticDecision::Append)
    }
}

/// Custom agentic decision maker utilizing a closure (for LLM model-based chunking)
pub struct CustomAgenticDecisionMaker<F>
where
    F: Fn(&[&str], &str) -> Result<AgenticDecision, ChunkrError> + Send + Sync,
{
    func: F,
}

impl<F> CustomAgenticDecisionMaker<F>
where
    F: Fn(&[&str], &str) -> Result<AgenticDecision, ChunkrError> + Send + Sync,
{
    pub fn new(func: F) -> Self {
        Self { func }
    }
}

impl<F> AgenticDecisionMaker for CustomAgenticDecisionMaker<F>
where
    F: Fn(&[&str], &str) -> Result<AgenticDecision, ChunkrError> + Send + Sync,
{
    fn decide(
        &self,
        current_chunk_sentences: &[&str],
        next_sentence: &str,
    ) -> Result<AgenticDecision, ChunkrError> {
        (self.func)(current_chunk_sentences, next_sentence)
    }
}

/// Autonomous agentic chunker that reviews sentences iteratively and evaluates boundary decisions
pub struct AgenticChunker {
    pub decision_maker: Arc<dyn AgenticDecisionMaker>,
}

impl std::fmt::Debug for AgenticChunker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgenticChunker").finish()
    }
}

impl Clone for AgenticChunker {
    fn clone(&self) -> Self {
        Self {
            decision_maker: Arc::clone(&self.decision_maker),
        }
    }
}

impl AgenticChunker {
    /// Create a new AgenticChunker with HeuristicAgenticDecisionMaker
    pub fn new() -> Self {
        Self {
            decision_maker: Arc::new(HeuristicAgenticDecisionMaker::new()),
        }
    }

    /// Use a custom decision maker (e.g. LLM agent)
    pub fn with_decision_maker(mut self, maker: impl AgenticDecisionMaker + 'static) -> Self {
        self.decision_maker = Arc::new(maker);
        self
    }
}

impl Default for AgenticChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunker for AgenticChunker {
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError> {
        if text.trim().is_empty() {
            return Err(ChunkrError::EmptyInput);
        }

        let sentences = SentenceChunker::split_sentences(text);
        if sentences.is_empty() {
            return Err(ChunkrError::EmptyInput);
        }

        let mut result = Vec::new();
        let mut current_sentences: Vec<&str> = Vec::new();
        let mut current_topic = "Initial Topic".to_string();
        let mut current_reason = "Document start".to_string();
        let mut chunk_idx = 0;

        for &sentence in &sentences {
            let decision = self.decision_maker.decide(&current_sentences, sentence)?;

            match decision {
                AgenticDecision::Append => {
                    current_sentences.push(sentence);
                }
                AgenticDecision::SplitAndStartNew { topic_label, reason } => {
                    if !current_sentences.is_empty() {
                        let content = current_sentences.join(" ");
                        let mut metadata = HashMap::with_capacity(5);
                        metadata.insert("length".to_string(), Value::from(content.len()));
                        metadata.insert("sentence_count".to_string(), Value::from(current_sentences.len()));
                        metadata.insert("topic_label".to_string(), Value::from(current_topic));
                        metadata.insert("split_reason".to_string(), Value::from(current_reason));
                        metadata.insert("chunk_index".to_string(), Value::from(chunk_idx));

                        result.push(Document { content, metadata });
                        chunk_idx += 1;
                        current_sentences.clear();
                    }

                    current_topic = topic_label;
                    current_reason = reason;
                    current_sentences.push(sentence);
                }
            }
        }

        if !current_sentences.is_empty() {
            let content = current_sentences.join(" ");
            let mut metadata = HashMap::with_capacity(5);
            metadata.insert("length".to_string(), Value::from(content.len()));
            metadata.insert("sentence_count".to_string(), Value::from(current_sentences.len()));
            metadata.insert("topic_label".to_string(), Value::from(current_topic));
            metadata.insert("split_reason".to_string(), Value::from(current_reason));
            metadata.insert("chunk_index".to_string(), Value::from(chunk_idx));

            result.push(Document { content, metadata });
        }

        Ok(result)
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for AgenticChunker {
    fn chunk_text(
        &self,
        text: &str,
        _chunk_size: usize,
        _overlap: usize,
    ) -> Result<Vec<Document>, String> {
        self.chunk(text).map_err(|e| e.to_string())
    }
}
