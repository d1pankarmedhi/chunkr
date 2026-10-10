//! Backend-neutral parse payload.
//!
//! Every high-fidelity backend (the native `pdf` feature, a Python plugin)
//! produces this shape; the mapping and sanitizing code below it is written
//! once against it.

use serde::{Deserialize, Serialize};

/// Bounding box in PDF points, top-left origin.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Bbox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// One table cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CellPayload {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<Bbox>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colspan: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rowspan: Option<u16>,
}

/// One classified layout block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct BlockPayload {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    pub bold: bool,
    pub italic: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ordered: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<Vec<CellPayload>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<Vec<CellPayload>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<Bbox>,
}

/// Per-page complexity signals, passed through when `include_complexity` is set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ComplexityPayload {
    pub needs_ocr: bool,
    pub reasons: Vec<String>,
    pub text_length: usize,
    pub text_coverage: f32,
    pub full_page_image: bool,
    pub is_garbled: bool,
    pub layout: Option<LayoutPayload>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LayoutPayload {
    pub column_count: usize,
    pub ruled_table_count: usize,
    pub text_table_run_count: usize,
    pub figure_count: usize,
    pub figure_coverage: f32,
    pub is_complex: bool,
    pub reasons: Vec<String>,
}

/// One parsed page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PagePayload {
    #[serde(alias = "page_num")]
    pub page_number: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_label: Option<String>,
    pub page_width: f32,
    pub page_height: f32,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,
    pub blocks: Vec<BlockPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub complexity: Option<ComplexityPayload>,
}

/// Block kinds the mapper understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Heading,
    Paragraph,
    ListItem,
    Table,
    Code,
    Rule,
    Figure,
    /// Anything a newer backend may add; treated as text.
    Other,
}

impl BlockKind {
    pub fn parse(kind: &str) -> Self {
        match kind {
            "heading" => Self::Heading,
            "paragraph" => Self::Paragraph,
            "list_item" => Self::ListItem,
            "table" | "merged_table" | "grid_fallback" => Self::Table,
            "code" => Self::Code,
            "rule" => Self::Rule,
            "figure" => Self::Figure,
            _ => Self::Other,
        }
    }
}

impl BlockPayload {
    pub fn kind(&self) -> BlockKind {
        BlockKind::parse(&self.kind)
    }

    /// Visible text of the block, regardless of kind.
    pub fn text_content(&self) -> String {
        match self.kind() {
            BlockKind::Table => self.table_text(),
            _ => {
                let mut out = self.text.clone().unwrap_or_default();
                if out.is_empty() {
                    if let Some(lines) = &self.lines {
                        out = lines.join("\n");
                    }
                }
                out
            }
        }
    }

    /// Table rows flattened into reading order, rows separated by newlines.
    pub fn table_text(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        for row in self.table_rows() {
            lines.push(row.join(" "));
        }
        if lines.is_empty() {
            if let Some(lines) = &self.lines {
                return lines.join("\n");
            }
        }
        lines.join("\n")
    }

    /// All non-empty cells in reading order, joined into a single line.
    /// Used when a rejected table is re-flowed into a paragraph.
    pub fn table_text_flat(&self) -> String {
        let cells: Vec<String> = self
            .table_rows()
            .iter()
            .flat_map(|row| row.iter().cloned())
            .filter(|c| !c.is_empty())
            .collect();
        cells.join(" ")
    }

    /// All rows including the header, cells flattened to strings.
    pub fn table_rows(&self) -> Vec<Vec<String>> {
        let mut rows: Vec<Vec<String>> = Vec::new();
        if let Some(header) = &self.header {
            if !header.is_empty() {
                rows.push(header.iter().map(|c| c.text.trim().to_string()).collect());
            }
        }
        if let Some(body) = &self.rows {
            for row in body {
                rows.push(row.iter().map(|c| c.text.trim().to_string()).collect());
            }
        }
        rows
    }

    /// Header cells, empty when the backend found none.
    pub fn header_cells(&self) -> Vec<String> {
        self.header
            .as_ref()
            .map(|h| h.iter().map(|c| c.text.trim().to_string()).collect())
            .unwrap_or_default()
    }

    pub fn has_header(&self) -> bool {
        self.header_cells().iter().any(|c| !c.is_empty())
    }

    /// Mutable view over every table cell (header first, then body).
    pub fn table_cells_mut(&mut self) -> impl Iterator<Item = &mut CellPayload> {
        self.header
            .iter_mut()
            .flatten()
            .chain(self.rows.iter_mut().flatten().flatten())
    }
}

/// Median word count per non-empty table cell, `0.0` for headerless tables.
pub fn median_words_per_cell(block: &BlockPayload) -> f32 {
    let mut counts: Vec<usize> = block
        .table_rows()
        .iter()
        .flat_map(|r| r.iter())
        .filter(|c| !c.is_empty())
        .map(|c| c.split_whitespace().count())
        .collect();
    if counts.is_empty() {
        return 0.0;
    }
    counts.sort_unstable();
    let mid = counts.len() / 2;
    if counts.len().is_multiple_of(2) {
        (counts[mid - 1] + counts[mid]) as f32 / 2.0
    } else {
        counts[mid] as f32
    }
}
