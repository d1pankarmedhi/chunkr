use crate::structures::document::Document;

/// Quality filter that drops residual, low-information, or corrupted chunks.
#[derive(Debug, Clone)]
pub struct ChunkFilter {
    pub min_characters: Option<usize>,
    pub max_characters: Option<usize>,
    pub min_words: Option<usize>,
    pub min_alpha_ratio: Option<f32>,
}

impl ChunkFilter {
    /// Create a new ChunkFilter with no default restrictions
    pub fn new() -> Self {
        Self {
            min_characters: None,
            max_characters: None,
            min_words: None,
            min_alpha_ratio: None,
        }
    }

    /// Set minimum character count (drops tiny debris chunks)
    pub fn with_min_characters(mut self, min: usize) -> Self {
        self.min_characters = Some(min);
        self
    }

    /// Set maximum character count
    pub fn with_max_characters(mut self, max: usize) -> Self {
        self.max_characters = Some(max);
        self
    }

    /// Set minimum word count (drops non-grammatical single-word snippets)
    pub fn with_min_words(mut self, min: usize) -> Self {
        self.min_words = Some(min);
        self
    }

    /// Set minimum alphanumeric ratio (e.g. 0.5 requires at least 50% letters/digits, dropping binary noise)
    pub fn with_min_alpha_ratio(mut self, ratio: f32) -> Self {
        self.min_alpha_ratio = Some(ratio.clamp(0.0, 1.0));
        self
    }

    /// Check if a document passes all configured quality criteria
    pub fn is_valid(&self, doc: &Document) -> bool {
        let text = doc.content.trim();
        let char_len = text.chars().count();

        if let Some(min_c) = self.min_characters {
            if char_len < min_c {
                return false;
            }
        }

        if let Some(max_c) = self.max_characters {
            if char_len > max_c {
                return false;
            }
        }

        if let Some(min_w) = self.min_words {
            let word_count = text.split_whitespace().count();
            if word_count < min_w {
                return false;
            }
        }

        if let Some(min_ratio) = self.min_alpha_ratio {
            if char_len > 0 {
                let alpha_count = text.chars().filter(|c| c.is_alphanumeric()).count();
                let ratio = alpha_count as f32 / char_len as f32;
                if ratio < min_ratio {
                    return false;
                }
            } else {
                return false;
            }
        }

        true
    }

    /// Filter a slice of documents, returning only valid chunks
    pub fn filter(&self, docs: &[Document]) -> Vec<Document> {
        docs.iter()
            .filter(|doc| self.is_valid(doc))
            .cloned()
            .collect()
    }
}

impl Default for ChunkFilter {
    fn default() -> Self {
        Self::new()
    }
}
