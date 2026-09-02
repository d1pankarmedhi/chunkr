use std::collections::HashMap;
use serde_json::Value;
use tree_sitter::{Node, Parser};

use crate::chunker::base::{BaseChunker, Chunker};
use crate::chunker::recursive::RecursiveChunker;
use crate::error::ChunkrError;
use crate::structures::document::Document;

/// Languages supported by the AST-based code chunker
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AstLanguage {
    #[default]
    Rust,
    Python,
}

impl AstLanguage {
    pub fn as_str(&self) -> &'static str {
        match self {
            AstLanguage::Rust => "rust",
            AstLanguage::Python => "python",
        }
    }
}

/// AST-based syntax-aware code chunker powered by Tree-sitter.
///
/// Ensures chunks strictly align with AST boundaries (functions, classes, structs, impls)
/// rather than naive character or regex splits.
#[derive(Debug, Clone)]
pub struct AstCodeChunker {
    pub language: AstLanguage,
    pub max_chunk_size: usize,
    sub_chunker: RecursiveChunker,
}

impl AstCodeChunker {
    /// Create a new AstCodeChunker for the specified language
    pub fn new(language: AstLanguage) -> Self {
        Self {
            language,
            max_chunk_size: 1500,
            sub_chunker: RecursiveChunker::new()
                .with_chunk_size(1500)
                .with_overlap(150),
        }
    }

    /// Set maximum chunk size in characters
    pub fn with_max_chunk_size(mut self, max_size: usize) -> Self {
        self.max_chunk_size = max_size;
        self.sub_chunker = self.sub_chunker.with_chunk_size(max_size);
        self
    }

    fn init_parser(&self) -> Result<Parser, ChunkrError> {
        let mut parser = Parser::new();
        let lang = match self.language {
            AstLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
            AstLanguage::Python => tree_sitter_python::LANGUAGE.into(),
        };
        parser
            .set_language(&lang)
            .map_err(|e| ChunkrError::ParseError(e.to_string()))?;
        Ok(parser)
    }

    fn is_definition_node(&self, kind: &str) -> bool {
        match self.language {
            AstLanguage::Rust => matches!(
                kind,
                "function_item"
                    | "struct_item"
                    | "enum_item"
                    | "impl_item"
                    | "trait_item"
                    | "mod_item"
                    | "macro_definition"
            ),
            AstLanguage::Python => matches!(
                kind,
                "function_definition" | "class_definition" | "decorated_definition"
            ),
        }
    }

    fn extract_node_name<'a>(&self, node: Node<'a>, source: &'a str) -> Option<String> {
        // If decorated definition, check internal function or class
        if node.kind() == "decorated_definition" {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.kind() == "function_definition" || child.kind() == "class_definition" {
                        return self.extract_node_name(child, source);
                    }
                }
            }
        }

        node.child_by_field_name("name")
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            .map(|s| s.to_string())
    }

    fn extract_node_type(&self, kind: &str) -> &'static str {
        match kind {
            "function_item" | "function_definition" => "function",
            "class_definition" => "class",
            "struct_item" => "struct",
            "enum_item" => "enum",
            "impl_item" => "impl",
            "trait_item" => "trait",
            "mod_item" => "mod",
            "decorated_definition" => "decorated",
            _ => "block",
        }
    }
}

impl Default for AstCodeChunker {
    fn default() -> Self {
        Self::new(AstLanguage::Rust)
    }
}

impl Chunker for AstCodeChunker {
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError> {
        if text.trim().is_empty() {
            return Err(ChunkrError::EmptyInput);
        }

        let mut parser = self.init_parser()?;
        let tree = parser
            .parse(text, None)
            .ok_or_else(|| ChunkrError::ParseError("Failed to parse AST".to_string()))?;

        let root_node = tree.root_node();
        let child_count = root_node.child_count();

        if child_count == 0 {
            return self.sub_chunker.chunk(text);
        }

        let mut result = Vec::new();
        let mut current_prelude = String::new();
        let mut prelude_start_line = 1;
        let mut chunk_idx = 0;

        let flush_prelude = |prelude: &mut String,
                             start_line: usize,
                             end_line: usize,
                             res: &mut Vec<Document>,
                             idx: &mut usize| {
            let trimmed = prelude.trim();
            if !trimmed.is_empty() {
                let mut metadata = HashMap::new();
                metadata.insert("language".to_string(), Value::from(self.language.as_str()));
                metadata.insert("node_type".to_string(), Value::from("prelude"));
                metadata.insert("start_line".to_string(), Value::from(start_line));
                metadata.insert("end_line".to_string(), Value::from(end_line));
                metadata.insert("length".to_string(), Value::from(trimmed.len()));
                metadata.insert("chunk_index".to_string(), Value::from(*idx));
                res.push(Document {
                    content: trimmed.to_string(),
                    metadata,
                });
                *idx += 1;
            }
            prelude.clear();
        };

        for i in 0..child_count {
            let child = match root_node.child(i) {
                Some(c) => c,
                None => continue,
            };

            let kind = child.kind();
            let start_byte = child.start_byte();
            let end_byte = child.end_byte();

            if start_byte >= text.len() || end_byte > text.len() || start_byte >= end_byte {
                continue;
            }

            let node_text = &text[start_byte..end_byte];
            let start_line = child.start_position().row + 1;
            let end_line = child.end_position().row + 1;

            if self.is_definition_node(kind) {
                // Flush accumulated prelude before definition
                flush_prelude(
                    &mut current_prelude,
                    prelude_start_line,
                    start_line.saturating_sub(1),
                    &mut result,
                    &mut chunk_idx,
                );

                let node_name = self.extract_node_name(child, text);
                let node_type = self.extract_node_type(kind);

                if node_text.len() <= self.max_chunk_size {
                    let mut metadata = HashMap::new();
                    metadata.insert("language".to_string(), Value::from(self.language.as_str()));
                    metadata.insert("node_type".to_string(), Value::from(node_type));
                    if let Some(name) = node_name {
                        metadata.insert("node_name".to_string(), Value::from(name));
                    }
                    metadata.insert("start_line".to_string(), Value::from(start_line));
                    metadata.insert("end_line".to_string(), Value::from(end_line));
                    metadata.insert("length".to_string(), Value::from(node_text.len()));
                    metadata.insert("chunk_index".to_string(), Value::from(chunk_idx));

                    result.push(Document {
                        content: node_text.to_string(),
                        metadata,
                    });
                    chunk_idx += 1;
                } else {
                    // Sub-split oversized definition
                    let sub_chunks = self.sub_chunker.chunk(node_text)?;
                    for sub in sub_chunks {
                        let mut metadata = sub.metadata;
                        metadata.insert("language".to_string(), Value::from(self.language.as_str()));
                        metadata.insert("node_type".to_string(), Value::from(node_type));
                        if let Some(ref name) = node_name {
                            metadata.insert("node_name".to_string(), Value::from(name.clone()));
                        }
                        metadata.insert("start_line".to_string(), Value::from(start_line));
                        metadata.insert("end_line".to_string(), Value::from(end_line));
                        metadata.insert("chunk_index".to_string(), Value::from(chunk_idx));

                        result.push(Document {
                            content: sub.content,
                            metadata,
                        });
                        chunk_idx += 1;
                    }
                }

                prelude_start_line = end_line + 1;
            } else {
                if current_prelude.is_empty() {
                    prelude_start_line = start_line;
                }
                current_prelude.push_str(node_text);
                current_prelude.push('\n');
            }
        }

        // Flush remaining trailing prelude/code
        let text_lines = text.lines().count();
        flush_prelude(
            &mut current_prelude,
            prelude_start_line,
            text_lines,
            &mut result,
            &mut chunk_idx,
        );

        if result.is_empty() {
            return self.sub_chunker.chunk(text);
        }

        Ok(result)
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for AstCodeChunker {
    fn chunk_text(
        &self,
        text: &str,
        chunk_size: usize,
        _overlap: usize,
    ) -> Result<Vec<Document>, String> {
        let mut cloned = self.clone();
        cloned.max_chunk_size = chunk_size;
        cloned.chunk(text).map_err(|e| e.to_string())
    }
}
