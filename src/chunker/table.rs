use std::collections::HashMap;
use serde_json::Value;

use crate::chunker::base::{BaseChunker, Chunker};
use crate::chunker::recursive::RecursiveChunker;
use crate::error::ChunkrError;
use crate::structures::document::Document;

/// Supported tabular formats for TableChunker
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TableFormat {
    /// Auto-detect Markdown table, CSV, or TSV based on content
    #[default]
    Auto,
    /// Markdown pipe tables (| col1 | col2 |)
    Markdown,
    /// Comma-separated values
    Csv,
    /// Tab-separated values
    Tsv,
}

impl TableFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            TableFormat::Auto => "auto",
            TableFormat::Markdown => "markdown",
            TableFormat::Csv => "csv",
            TableFormat::Tsv => "tsv",
        }
    }
}

/// Table-aware chunker that preserves column headers and table structure across all chunks.
///
/// Ensures that downstream LLMs and vector embeddings never lose column schema context
/// when tabular data spans multiple chunks.
#[derive(Debug, Clone)]
pub struct TableChunker {
    pub chunk_size: usize,
    pub rows_per_chunk: Option<usize>,
    pub overlap_rows: usize,
    pub format: TableFormat,
    sub_chunker: RecursiveChunker,
}

impl TableChunker {
    /// Create a new TableChunker with default settings:
    /// - `chunk_size`: 1000 characters
    /// - `rows_per_chunk`: None (sized by character budget)
    /// - `overlap_rows`: 1 row
    /// - `format`: TableFormat::Auto
    pub fn new() -> Self {
        Self {
            chunk_size: 1000,
            rows_per_chunk: None,
            overlap_rows: 1,
            format: TableFormat::Auto,
            sub_chunker: RecursiveChunker::new()
                .with_chunk_size(1000)
                .with_overlap(150),
        }
    }

    /// Set maximum character size for each chunk (when rows_per_chunk is None)
    pub fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = chunk_size;
        self.sub_chunker = self.sub_chunker.with_chunk_size(chunk_size);
        self
    }

    /// Set explicit maximum data rows per chunk
    pub fn with_rows_per_chunk(mut self, rows: Option<usize>) -> Self {
        self.rows_per_chunk = rows;
        self
    }

    /// Set row overlap between consecutive chunks (default: 1)
    pub fn with_overlap_rows(mut self, overlap: usize) -> Self {
        self.overlap_rows = overlap;
        self
    }

    /// Set table format (Auto, Markdown, Csv, Tsv)
    pub fn with_format(mut self, format: TableFormat) -> Self {
        self.format = format;
        self
    }

    /// Helper to detect table format from text content
    fn detect_format(text: &str) -> TableFormat {
        let lines: Vec<&str> = text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
        if lines.is_empty() {
            return TableFormat::Markdown;
        }

        // Check for Markdown table: header line with '|' followed by delimiter line with '---'
        for i in 0..lines.len().saturating_sub(1) {
            if is_markdown_table_row(lines[i]) && is_markdown_delimiter_row(lines[i + 1]) {
                return TableFormat::Markdown;
            }
        }

        // Check for TSV or CSV in first non-empty line
        let first = lines[0];
        let tab_count = first.matches('\t').count();
        let comma_count = first.matches(',').count();

        if tab_count > 0 && tab_count >= comma_count {
            TableFormat::Tsv
        } else if comma_count > 0 {
            TableFormat::Csv
        } else {
            TableFormat::Markdown
        }
    }
}

impl Default for TableChunker {
    fn default() -> Self {
        Self::new()
    }
}

/// Checks whether a line matches a Markdown table row format
fn is_markdown_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.contains('|') && !trimmed.starts_with('#')
}

/// Checks whether a line matches a Markdown table delimiter row (e.g. `|---|---|` or `|:---|---:|`)
fn is_markdown_delimiter_row(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.contains('|') || !trimmed.contains('-') {
        return false;
    }
    let inner = trimmed.trim_matches('|');
    inner.split('|').all(|col| {
        let c = col.trim();
        !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')
    })
}

/// Parse column names from a Markdown table header row
fn parse_markdown_columns(header_line: &str) -> Vec<String> {
    header_line
        .trim()
        .trim_matches('|')
        .split('|')
        .map(|col| col.trim().to_string())
        .filter(|col| !col.is_empty())
        .collect()
}

/// Parse column names from a CSV or TSV header row
fn parse_delimited_columns(header_line: &str, delimiter: char) -> Vec<String> {
    header_line
        .split(delimiter)
        .map(|col| col.trim().trim_matches('"').to_string())
        .collect()
}

#[derive(Debug)]
enum DocumentBlock<'a> {
    Text(&'a str),
    MarkdownTable {
        header: &'a str,
        delimiter: &'a str,
        rows: Vec<&'a str>,
    },
}

impl TableChunker {
    /// Chunk a Markdown table into sub-tables with repeated headers
    fn chunk_markdown_table(
        &self,
        header: &str,
        delimiter: &str,
        rows: &[&str],
        global_chunk_idx: &mut usize,
    ) -> Vec<Document> {
        let columns = parse_markdown_columns(header);
        let total_rows = rows.len();

        if total_rows == 0 {
            let mut metadata = HashMap::new();
            metadata.insert("is_table".to_string(), Value::from(true));
            metadata.insert("format".to_string(), Value::from("markdown"));
            metadata.insert("columns".to_string(), serde_json::to_value(&columns).unwrap_or(Value::Null));
            metadata.insert("start_row".to_string(), Value::from(0));
            metadata.insert("end_row".to_string(), Value::from(0));
            metadata.insert("total_rows".to_string(), Value::from(0));
            metadata.insert("chunk_index".to_string(), Value::from(*global_chunk_idx));
            *global_chunk_idx += 1;

            let content = format!("{}\n{}", header, delimiter);
            return vec![Document { content, metadata }];
        }

        let header_prefix = format!("{}\n{}\n", header, delimiter);
        let header_len = header_prefix.len();

        let mut chunks = Vec::new();
        let mut row_idx = 0;

        while row_idx < total_rows {
            let mut current_rows: Vec<&str> = Vec::new();
            let mut current_len = header_len;
            let start_row_num = row_idx + 1; // 1-based index

            while row_idx < total_rows {
                let row = rows[row_idx];
                let row_len = row.len() + 1; // including newline

                if let Some(max_rows) = self.rows_per_chunk {
                    if current_rows.len() >= max_rows {
                        break;
                    }
                } else if !current_rows.is_empty() && (current_len + row_len > self.chunk_size) {
                    break;
                }

                current_rows.push(row);
                current_len += row_len;
                row_idx += 1;
            }

            let end_row_num = start_row_num + current_rows.len().saturating_sub(1);
            let mut content = header_prefix.clone();
            content.push_str(&current_rows.join("\n"));

            let mut metadata = HashMap::new();
            metadata.insert("is_table".to_string(), Value::from(true));
            metadata.insert("format".to_string(), Value::from("markdown"));
            metadata.insert("columns".to_string(), serde_json::to_value(&columns).unwrap_or(Value::Null));
            metadata.insert("start_row".to_string(), Value::from(start_row_num));
            metadata.insert("end_row".to_string(), Value::from(end_row_num));
            metadata.insert("total_rows".to_string(), Value::from(total_rows));
            metadata.insert("chunk_index".to_string(), Value::from(*global_chunk_idx));
            *global_chunk_idx += 1;

            chunks.push(Document { content, metadata });

            // Apply overlap if there are remaining rows
            if row_idx < total_rows {
                let overlap = if let Some(max_rows) = self.rows_per_chunk {
                    self.overlap_rows.min(max_rows.saturating_sub(1))
                } else {
                    self.overlap_rows.min(current_rows.len().saturating_sub(1))
                };
                if overlap > 0 && row_idx >= overlap {
                    row_idx -= overlap;
                }
            }
        }

        chunks
    }

    /// Chunk a delimited table (CSV or TSV) into sub-tables with repeated headers
    fn chunk_delimited_table(
        &self,
        text: &str,
        delimiter: char,
        format_str: &'static str,
        global_chunk_idx: &mut usize,
    ) -> Result<Vec<Document>, ChunkrError> {
        let lines: Vec<&str> = text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
        if lines.is_empty() {
            return Err(ChunkrError::EmptyInput);
        }

        let header = lines[0];
        let data_rows = &lines[1..];
        let columns = parse_delimited_columns(header, delimiter);
        let total_rows = data_rows.len();

        if total_rows == 0 {
            let mut metadata = HashMap::new();
            metadata.insert("is_table".to_string(), Value::from(true));
            metadata.insert("format".to_string(), Value::from(format_str));
            metadata.insert("columns".to_string(), serde_json::to_value(&columns).unwrap_or(Value::Null));
            metadata.insert("start_row".to_string(), Value::from(0));
            metadata.insert("end_row".to_string(), Value::from(0));
            metadata.insert("total_rows".to_string(), Value::from(0));
            metadata.insert("chunk_index".to_string(), Value::from(*global_chunk_idx));
            *global_chunk_idx += 1;

            return Ok(vec![Document {
                content: header.to_string(),
                metadata,
            }]);
        }

        let header_prefix = format!("{}\n", header);
        let header_len = header_prefix.len();

        let mut chunks = Vec::new();
        let mut row_idx = 0;

        while row_idx < total_rows {
            let mut current_rows: Vec<&str> = Vec::new();
            let mut current_len = header_len;
            let start_row_num = row_idx + 1;

            while row_idx < total_rows {
                let row = data_rows[row_idx];
                let row_len = row.len() + 1;

                if let Some(max_rows) = self.rows_per_chunk {
                    if current_rows.len() >= max_rows {
                        break;
                    }
                } else if !current_rows.is_empty() && (current_len + row_len > self.chunk_size) {
                    break;
                }

                current_rows.push(row);
                current_len += row_len;
                row_idx += 1;
            }

            let end_row_num = start_row_num + current_rows.len().saturating_sub(1);
            let mut content = header_prefix.clone();
            content.push_str(&current_rows.join("\n"));

            let mut metadata = HashMap::new();
            metadata.insert("is_table".to_string(), Value::from(true));
            metadata.insert("format".to_string(), Value::from(format_str));
            metadata.insert("columns".to_string(), serde_json::to_value(&columns).unwrap_or(Value::Null));
            metadata.insert("start_row".to_string(), Value::from(start_row_num));
            metadata.insert("end_row".to_string(), Value::from(end_row_num));
            metadata.insert("total_rows".to_string(), Value::from(total_rows));
            metadata.insert("chunk_index".to_string(), Value::from(*global_chunk_idx));
            *global_chunk_idx += 1;

            chunks.push(Document { content, metadata });

            if row_idx < total_rows {
                let overlap = if let Some(max_rows) = self.rows_per_chunk {
                    self.overlap_rows.min(max_rows.saturating_sub(1))
                } else {
                    self.overlap_rows.min(current_rows.len().saturating_sub(1))
                };
                if overlap > 0 && row_idx >= overlap {
                    row_idx -= overlap;
                }
            }
        }

        Ok(chunks)
    }

    /// Parse a document into text blocks and embedded Markdown table blocks
    fn parse_document_blocks<'a>(text: &'a str) -> Vec<DocumentBlock<'a>> {
        let lines: Vec<&str> = text.lines().collect();
        let mut blocks = Vec::new();
        let mut i = 0;
        let mut text_start = 0;
        let mut byte_offset = 0;

        while i < lines.len() {
            let line = lines[i];
            let line_len = line.len();

            // Look ahead for markdown table: line[i] has '|' and line[i+1] is delimiter row
            if i + 1 < lines.len()
                && is_markdown_table_row(line)
                && is_markdown_delimiter_row(lines[i + 1])
            {
                // Emit preceding text block if any
                if byte_offset > text_start {
                    let text_slice = text[text_start..byte_offset].trim();
                    if !text_slice.is_empty() {
                        blocks.push(DocumentBlock::Text(text_slice));
                    }
                }

                let header = line.trim();
                let delimiter = lines[i + 1].trim();
                let mut rows = Vec::new();
                let mut advance = 2;

                while i + advance < lines.len() {
                    let next_line = lines[i + advance];
                    let trimmed = next_line.trim();
                    if trimmed.is_empty() || !is_markdown_table_row(trimmed) {
                        break;
                    }
                    rows.push(trimmed);
                    advance += 1;
                }

                blocks.push(DocumentBlock::MarkdownTable {
                    header,
                    delimiter,
                    rows,
                });

                // Calculate bytes advanced by the table
                for step in 0..advance {
                    byte_offset += lines[i + step].len() + 1; // including '\n'
                }
                byte_offset = byte_offset.min(text.len());
                text_start = byte_offset;
                i += advance;
                continue;
            }

            byte_offset += line_len + 1;
            byte_offset = byte_offset.min(text.len());
            i += 1;
        }

        if text_start < text.len() {
            let remaining = text[text_start..].trim();
            if !remaining.is_empty() {
                blocks.push(DocumentBlock::Text(remaining));
            }
        }

        blocks
    }
}

impl Chunker for TableChunker {
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError> {
        if text.trim().is_empty() {
            return Err(ChunkrError::EmptyInput);
        }

        let format = match self.format {
            TableFormat::Auto => Self::detect_format(text),
            f => f,
        };

        let mut global_chunk_idx = 0;

        match format {
            TableFormat::Csv => {
                self.chunk_delimited_table(text, ',', "csv", &mut global_chunk_idx)
            }
            TableFormat::Tsv => {
                self.chunk_delimited_table(text, '\t', "tsv", &mut global_chunk_idx)
            }
            TableFormat::Markdown | TableFormat::Auto => {
                let blocks = Self::parse_document_blocks(text);
                if blocks.is_empty() {
                    // Fallback to recursive sub-chunker
                    return self.sub_chunker.chunk(text);
                }

                let mut result = Vec::new();

                for block in blocks {
                    match block {
                        DocumentBlock::Text(prose) => {
                            if prose.len() <= self.chunk_size {
                                let mut metadata = HashMap::new();
                                metadata.insert("is_table".to_string(), Value::from(false));
                                metadata.insert("length".to_string(), Value::from(prose.len()));
                                metadata.insert("chunk_index".to_string(), Value::from(global_chunk_idx));
                                global_chunk_idx += 1;

                                result.push(Document {
                                    content: prose.to_string(),
                                    metadata,
                                });
                            } else {
                                let sub_docs = self.sub_chunker.chunk(prose)?;
                                for sub_doc in sub_docs {
                                    let mut metadata = sub_doc.metadata;
                                    metadata.insert("is_table".to_string(), Value::from(false));
                                    metadata.insert("chunk_index".to_string(), Value::from(global_chunk_idx));
                                    global_chunk_idx += 1;

                                    result.push(Document {
                                        content: sub_doc.content,
                                        metadata,
                                    });
                                }
                            }
                        }
                        DocumentBlock::MarkdownTable {
                            header,
                            delimiter,
                            rows,
                        } => {
                            let table_chunks = self.chunk_markdown_table(
                                header,
                                delimiter,
                                &rows,
                                &mut global_chunk_idx,
                            );
                            result.extend(table_chunks);
                        }
                    }
                }

                Ok(result)
            }
        }
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for TableChunker {
    fn chunk_text(
        &self,
        text: &str,
        chunk_size: usize,
        overlap: usize,
    ) -> Result<Vec<Document>, String> {
        let chunker = self
            .clone()
            .with_chunk_size(chunk_size)
            .with_overlap_rows(overlap);
        chunker.chunk(text).map_err(|e| e.to_string())
    }
}
