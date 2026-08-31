use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Document {
    pub content: String,
    pub metadata: HashMap<String, Value>,
}

impl Document {
    /// Create a new Document with content and metadata
    pub fn new(content: impl Into<String>, metadata: HashMap<String, Value>) -> Self {
        Self {
            content: content.into(),
            metadata,
        }
    }

    /// Create a Document from text content with empty metadata
    pub fn from_text(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            metadata: HashMap::new(),
        }
    }

    /// Add a key-value pair to metadata (builder pattern)
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Insert a key-value pair into metadata
    pub fn add_metadata(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        self.metadata.insert(key.into(), value.into());
    }

    /// Get a reference to the text content
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Get a reference to the metadata map
    pub fn metadata(&self) -> &HashMap<String, Value> {
        &self.metadata
    }

    /// Get mutable reference to metadata
    pub fn metadata_mut(&mut self) -> &mut HashMap<String, Value> {
        &mut self.metadata
    }
}
