//! Turn backend payloads into [`Document`]s, applying the configured output,
//! sanitizing and granularity rules.

use serde_json::{json, Value};
use std::collections::HashMap;

use crate::structures::document::Document;

use super::config::{
    ExtractConfig, FigureMode, Granularity, ParserConfig, ParserOutput, TableMode,
};
use super::payload::{BlockKind, BlockPayload, PagePayload};
use super::sanitize::{sanitize_pages, SanitizeReport};

/// Provenance stamped on every produced document.
#[derive(Debug, Clone, Default)]
pub struct SourceMeta {
    pub source: Option<String>,
    pub file_name: Option<String>,
    pub backend: String,
    pub parser_version: Option<String>,
}

/// Documents plus the report describing what the sanitizer changed.
#[derive(Debug, Clone, Default)]
pub struct ParseOutcome {
    pub documents: Vec<Document>,
    pub report: SanitizeReport,
    pub total_pages: usize,
}

/// Map parsed pages to documents. Sanitizing happens here so plugins cannot
/// skip it by accident.
pub fn pages_to_documents(
    pages: &[PagePayload],
    cfg: &ParserConfig,
    meta: &SourceMeta,
) -> ParseOutcome {
    let mut pages = pages.to_vec();
    let report = sanitize_pages(&mut pages, &cfg.sanitize);
    let total_pages = pages.len();

    let mut documents = match cfg.granularity {
        Granularity::Page => page_documents(&pages, cfg, meta, &report),
        Granularity::Block => block_documents(&pages, cfg, meta, &report),
        Granularity::Document => vec![document_document(&pages, cfg, meta, &report)],
    };

    // Drop empty documents but keep the provenance on the survivors.
    documents.retain(|doc| !doc.content.trim().is_empty());
    for doc in documents.iter_mut() {
        doc.metadata
            .entry("total_pages".to_string())
            .or_insert_with(|| Value::from(total_pages));
    }

    ParseOutcome {
        documents,
        report,
        total_pages,
    }
}

fn base_metadata(meta: &SourceMeta, report: &SanitizeReport) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    if let Some(source) = &meta.source {
        out.insert("source".to_string(), Value::from(source.clone()));
    }
    if let Some(file_name) = &meta.file_name {
        out.insert("file_name".to_string(), Value::from(file_name.clone()));
    }
    out.insert(
        "parser_backend".to_string(),
        Value::from(meta.backend.clone()),
    );
    if let Some(version) = &meta.parser_version {
        out.insert("parser_version".to_string(), Value::from(version.clone()));
    }
    out.insert(
        "parser_sanitized".to_string(),
        Value::from(report.tables_demoted + report.headings_demoted > 0),
    );
    if !report.is_clean() {
        if let Ok(value) = serde_json::to_value(report) {
            out.insert("sanitize_report".to_string(), value);
        }
    }
    out
}

fn page_metadata(page: &PagePayload, cfg: &ParserConfig) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    out.insert("page_number".to_string(), Value::from(page.page_number));
    if cfg.include_page_labels {
        if let Some(label) = &page.page_label {
            out.insert("page_label".to_string(), Value::from(label.clone()));
        }
    }
    if cfg.include_complexity {
        if let Some(complexity) = &page.complexity {
            if let Ok(value) = serde_json::to_value(complexity) {
                out.insert("pdf_complexity".to_string(), value);
            }
        }
    }
    out
}

fn page_content(page: &PagePayload, cfg: &ParserConfig) -> String {
    match cfg.output {
        ParserOutput::Text => {
            if page.text.trim().is_empty() {
                render_markdown(&page.blocks, &cfg.extract)
            } else {
                page.text.clone()
            }
        }
        ParserOutput::Markdown | ParserOutput::Both => match (cfg.sanitize.enabled, &page.markdown)
        {
            // Raw passthrough: the backend markdown is kept byte for byte.
            (false, Some(markdown)) if !markdown.trim().is_empty() => markdown.clone(),
            _ => render_markdown(&page.blocks, &cfg.extract),
        },
    }
}

fn page_documents(
    pages: &[PagePayload],
    cfg: &ParserConfig,
    meta: &SourceMeta,
    report: &SanitizeReport,
) -> Vec<Document> {
    pages
        .iter()
        .map(|page| {
            let mut metadata = base_metadata(meta, report);
            metadata.extend(page_metadata(page, cfg));
            metadata.insert(
                "parser_output".to_string(),
                Value::from(output_name(cfg.output)),
            );
            if matches!(cfg.output, ParserOutput::Both) {
                metadata.insert("text".to_string(), Value::from(page.text.clone()));
            }
            if cfg.keep_blocks && !page.blocks.is_empty() {
                if let Ok(value) = serde_json::to_value(&page.blocks) {
                    metadata.insert("blocks".to_string(), value);
                }
            }
            Document::new(page_content(page, cfg), metadata)
        })
        .collect()
}

fn document_document(
    pages: &[PagePayload],
    cfg: &ParserConfig,
    meta: &SourceMeta,
    report: &SanitizeReport,
) -> Document {
    let mut metadata = base_metadata(meta, report);
    metadata.insert(
        "parser_output".to_string(),
        Value::from(output_name(cfg.output)),
    );
    if cfg.include_complexity {
        let complexity: Vec<Value> = pages
            .iter()
            .filter_map(|p| p.complexity.as_ref())
            .filter_map(|c| serde_json::to_value(c).ok())
            .collect();
        if !complexity.is_empty() {
            metadata.insert("pdf_complexity".to_string(), Value::from(complexity));
        }
    }
    let content = pages
        .iter()
        .map(|page| page_content(page, cfg))
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    Document::new(content, metadata)
}

fn block_documents(
    pages: &[PagePayload],
    cfg: &ParserConfig,
    meta: &SourceMeta,
    report: &SanitizeReport,
) -> Vec<Document> {
    let mut documents = Vec::new();
    for page in pages {
        for block in &page.blocks {
            let content = block_content(block, cfg);
            if content.trim().is_empty() {
                continue;
            }
            let mut metadata = base_metadata(meta, report);
            metadata.extend(page_metadata(page, cfg));
            metadata.insert(
                "parser_output".to_string(),
                Value::from(output_name(cfg.output)),
            );
            metadata.insert("block_type".to_string(), Value::from(block.kind.clone()));
            if let Some(level) = block.level {
                metadata.insert("heading_level".to_string(), Value::from(level));
            }
            if let Some(marker) = &block.marker {
                metadata.insert("list_marker".to_string(), Value::from(marker.clone()));
            }
            if let Some(ordered) = block.ordered {
                metadata.insert("ordered".to_string(), Value::from(ordered));
            }
            if block.bold {
                metadata.insert("bold".to_string(), Value::from(true));
            }
            if block.kind() == BlockKind::Table {
                let rows = block.table_rows();
                metadata.insert("table_header".to_string(), json!(block.header_cells()));
                metadata.insert("table_rows".to_string(), json!(rows));
            }
            if cfg.include_bboxes {
                if let Some(bbox) = block.bbox {
                    metadata.insert(
                        "bbox".to_string(),
                        json!([bbox.x, bbox.y, bbox.width, bbox.height]),
                    );
                }
            }
            if cfg.keep_blocks {
                if let Ok(value) = serde_json::to_value(block) {
                    metadata.insert("block".to_string(), value);
                }
            }
            documents.push(Document::new(content, metadata));
        }
    }
    documents
}

fn block_content(block: &BlockPayload, cfg: &ParserConfig) -> String {
    match cfg.output {
        ParserOutput::Text => block.text_content(),
        ParserOutput::Markdown | ParserOutput::Both => {
            render_blocks(std::slice::from_ref(block), &cfg.extract)
        }
    }
}

fn output_name(output: ParserOutput) -> &'static str {
    match output {
        ParserOutput::Text => "text",
        ParserOutput::Markdown => "markdown",
        ParserOutput::Both => "both",
    }
}

/// Render layout blocks as markdown.
pub fn render_markdown(blocks: &[BlockPayload], extract: &ExtractConfig) -> String {
    render_blocks(blocks, extract)
}

fn render_blocks(blocks: &[BlockPayload], extract: &ExtractConfig) -> String {
    let mut out: Vec<String> = Vec::with_capacity(blocks.len());
    for block in blocks {
        let rendered = match block.kind() {
            BlockKind::Heading => {
                let text = block.text.clone().unwrap_or_default();
                if !extract.headings || text.trim().is_empty() {
                    text
                } else {
                    let level = block.level.unwrap_or(2).clamp(1, 6) as usize;
                    format!("{} {}", "#".repeat(level), text.trim())
                }
            }
            BlockKind::ListItem => {
                let text = block.text.clone().unwrap_or_default();
                if !extract.lists || text.trim().is_empty() {
                    text
                } else {
                    let marker = match (block.ordered.unwrap_or(false), &block.marker) {
                        (true, Some(marker)) if !marker.is_empty() => marker.clone(),
                        (true, _) => "1.".to_string(),
                        (false, Some(marker)) if !marker.is_empty() => marker.clone(),
                        (false, _) => "-".to_string(),
                    };
                    format!("{} {}", marker, text.trim())
                }
            }
            BlockKind::Table => match extract.tables {
                TableMode::Off => block.table_text(),
                _ => table_markdown(block),
            },
            BlockKind::Code => {
                let body = block.text_content();
                let lang = block.lang.clone().unwrap_or_default();
                format!("```{lang}\n{body}\n```")
            }
            BlockKind::Rule => "---".to_string(),
            BlockKind::Figure => match extract.figures {
                FigureMode::Skip => String::new(),
                FigureMode::Placeholder => match &block.id {
                    Some(id) => format!("![]({id})"),
                    None => String::new(),
                },
                FigureMode::Link => match &block.id {
                    Some(id) => format!(
                        "![{}]({id})",
                        block.format.clone().unwrap_or_else(|| "figure".to_string())
                    ),
                    None => String::new(),
                },
            },
            _ => block.text_content(),
        };
        if !rendered.trim().is_empty() {
            out.push(rendered.trim_end().to_string());
        }
    }
    out.join("\n\n")
}

fn table_markdown(block: &BlockPayload) -> String {
    let rows = block.table_rows();
    if rows.is_empty() {
        return block.table_text();
    }
    let columns = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut lines = Vec::with_capacity(rows.len() + 1);
    for (index, row) in rows.iter().enumerate() {
        let mut cells: Vec<String> = row.iter().map(|cell| escape_cell(cell)).collect();
        cells.resize(columns.max(1), String::new());
        lines.push(format!("| {} |", cells.join(" | ")));
        if index == 0 {
            lines.push(format!("|{}|", " --- |".repeat(columns.max(1))));
        }
    }
    lines.join("\n")
}

fn escape_cell(cell: &str) -> String {
    cell.replace('|', "\\|").replace('\n', " ")
}
