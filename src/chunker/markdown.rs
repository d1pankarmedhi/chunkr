use std::collections::HashMap;
use serde_json::Value;

use crate::chunker::base::{BaseChunker, Chunker};
use crate::chunker::recursive::RecursiveChunker;
use crate::error::ChunkrError;
use crate::structures::document::Document;

/// Structure-aware chunker for Markdown files.
///
/// Features:
/// - Splits along header boundaries (`#` to `######`)
/// - Preserves full header hierarchy / breadcrumb paths in chunk metadata
/// - Guards fenced code blocks (``` / ~~~) from being broken across chunks
/// - Sub-splits oversized sections recursively while preserving header context
#[derive(Debug, Clone)]
pub struct MarkdownChunker {
    pub chunk_size: usize,
    pub overlap: usize,
    pub include_header_in_content: bool,
    sub_chunker: RecursiveChunker,
}

#[derive(Debug, Clone)]
struct MarkdownSection<'a> {
    pub headers: Vec<(usize, &'a str)>, // (level, title)
    pub content: &'a str,
    pub has_code_block: bool,
}

impl MarkdownChunker {
    pub fn new() -> Self {
        Self {
            chunk_size: 1000,
            overlap: 150,
            include_header_in_content: true,
            sub_chunker: RecursiveChunker::new()
                .with_chunk_size(1000)
                .with_overlap(150),
        }
    }

    pub fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = chunk_size;
        self.sub_chunker = self.sub_chunker.with_chunk_size(chunk_size);
        self
    }

    pub fn with_overlap(mut self, overlap: usize) -> Self {
        self.overlap = overlap;
        self.sub_chunker = self.sub_chunker.with_overlap(overlap);
        self
    }

    pub fn with_include_header_in_content(mut self, include: bool) -> Self {
        self.include_header_in_content = include;
        self
    }

    /// Zero-allocation slice-based markdown section parser
    fn parse_sections<'a>(&self, text: &'a str) -> Vec<MarkdownSection<'a>> {
        let mut sections: Vec<MarkdownSection<'a>> = Vec::new();
        let mut current_headers: Vec<(usize, &'a str)> = Vec::new();
        let mut section_start = 0;
        let mut section_has_code = false;
        let mut in_code_block = false;

        let mut byte_offset = 0;

        for line in text.split('\n') {
            let line_len = line.len();
            let trimmed = line.trim();

            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                in_code_block = !in_code_block;
                section_has_code = true;
                byte_offset += line_len + 1;
                continue;
            }

            if in_code_block {
                byte_offset += line_len + 1;
                continue;
            }

            if let Some((level, title)) = parse_header_slice(line) {
                if byte_offset > section_start {
                    let end_byte = (byte_offset - 1).min(text.len());
                    if end_byte > section_start {
                        let sec_text = &text[section_start..end_byte];
                        if !sec_text.trim().is_empty() {
                            sections.push(MarkdownSection {
                                headers: current_headers.clone(),
                                content: sec_text,
                                has_code_block: section_has_code,
                            });
                        }
                    }
                    section_start = byte_offset;
                    section_has_code = false;
                }

                while let Some(&(last_level, _)) = current_headers.last() {
                    if last_level >= level {
                        current_headers.pop();
                    } else {
                        break;
                    }
                }
                current_headers.push((level, title));
            }

            byte_offset += line_len + 1;
        }

        if section_start < text.len() {
            let sec_text = &text[section_start..];
            if !sec_text.trim().is_empty() {
                sections.push(MarkdownSection {
                    headers: current_headers,
                    content: sec_text,
                    has_code_block: section_has_code,
                });
            }
        }

        sections
    }
}

fn parse_header_slice(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }

    let mut level = 0;
    for ch in trimmed.chars() {
        if ch == '#' {
            level += 1;
        } else {
            break;
        }
    }

    if level > 0 && level <= 6 {
        let rest = trimmed[level..].trim_start();
        if trimmed.as_bytes().get(level) == Some(&b' ') || rest.is_empty() {
            return Some((level, rest));
        }
    }

    None
}

impl Default for MarkdownChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunker for MarkdownChunker {
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError> {
        if text.trim().is_empty() {
            return Err(ChunkrError::EmptyInput);
        }

        let sections = self.parse_sections(text);
        if sections.is_empty() {
            return self.sub_chunker.chunk(text);
        }

        let mut result = Vec::new();
        let mut chunk_idx = 0;

        for section in sections {
            let header_titles: Vec<String> = section.headers.iter().map(|(_, t)| t.to_string()).collect();
            let header_path = header_titles.join(" > ");

            if section.content.len() <= self.chunk_size {
                let trimmed = section.content.trim();
                if !trimmed.is_empty() {
                    let mut metadata = HashMap::new();
                    metadata.insert("length".to_string(), Value::from(trimmed.len()));
                    metadata.insert("headers".to_string(), serde_json::to_value(&header_titles).unwrap_or(Value::Null));
                    metadata.insert("header_path".to_string(), Value::from(header_path));
                    metadata.insert("has_code_block".to_string(), Value::from(section.has_code_block));
                    metadata.insert("chunk_index".to_string(), Value::from(chunk_idx));

                    result.push(Document {
                        content: trimmed.to_string(),
                        metadata,
                    });
                    chunk_idx += 1;
                }
            } else {
                // Sub-split oversized section
                let sub_docs = self.sub_chunker.chunk(section.content)?;
                for sub_doc in sub_docs {
                    let mut metadata = HashMap::new();
                    metadata.insert("length".to_string(), Value::from(sub_doc.content.len()));
                    metadata.insert("headers".to_string(), serde_json::to_value(&header_titles).unwrap_or(Value::Null));
                    metadata.insert("header_path".to_string(), Value::from(header_path.clone()));
                    metadata.insert("has_code_block".to_string(), Value::from(section.has_code_block));
                    metadata.insert("chunk_index".to_string(), Value::from(chunk_idx));

                    result.push(Document {
                        content: sub_doc.content,
                        metadata,
                    });
                    chunk_idx += 1;
                }
            }
        }

        Ok(result)
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for MarkdownChunker {
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
