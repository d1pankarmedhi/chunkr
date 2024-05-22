use std::collections::HashMap;

use serde_json::Value;

use super::super::structures::document::Document;
use super::base::BaseChunker;

pub struct CharacterChunker {}
impl CharacterChunker {
    pub fn new() -> Self {
        Self {}
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for CharacterChunker {
    fn chunk_text(
        &self,
        text: &str,
        chunk_size: usize,
        overlap: usize,
    ) -> Result<Vec<Document>, String> {
        let mut result: Vec<Document> = Vec::new();
        let mut temp_str = String::new();
        let mut start = 0;

        let chars: Vec<char> = text.chars().collect();

        if text.trim().is_empty() {
            return Err("Need to pass text".to_string());
        }

        while start < text.len() {
            if (start + chunk_size) >= chars.len() {
                break;
            }

            for i in 0..chunk_size {
                temp_str.push(chars[start + i]);
            }

            if !temp_str.is_empty() {
                let mut metadata = HashMap::new();
                metadata.insert("length".to_string(), Value::from(temp_str.len()));
                metadata.insert("source".to_string(), Value::from("source"));

                let doc = Document {
                    content: temp_str.trim_end().to_string(),
                    metadata: metadata,
                };
                result.push(doc);
                temp_str.clear();
            }

            start += chunk_size - overlap;
        }

        Ok(result)
    }
}
