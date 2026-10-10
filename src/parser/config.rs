//! Parser configuration shared by every PDF extraction backend.
//!
//! The config is plain data: it serializes to JSON so Python plugins and the
//! native `pdf` feature drive the same mapping code through the same knobs.

use serde::{Deserialize, Serialize};

/// What a produced [`Document`](crate::structures::document::Document) contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ParserOutput {
    /// Layout-preserving plain text (default).
    #[default]
    Text,
    /// Markdown rendered from layout blocks.
    Markdown,
    /// Markdown content plus the plain text under `metadata["text"]`.
    Both,
}

/// How one page is turned into documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Granularity {
    /// One document per page (default).
    #[default]
    Page,
    /// One document per layout block, tagged with `block_type`.
    Block,
    /// A single document for the whole file.
    Document,
}

/// PDF extraction backend selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    /// Use a registered high-fidelity backend when one is available, else the
    /// built-in fast extractor.
    #[default]
    Auto,
    /// Built-in `lopdf` extractor (fastest, no structure, drops unmapped glyphs).
    Fast,
    /// High-fidelity layout backend (native `pdf` feature or a Python plugin).
    Liteparse,
}

/// Table handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TableMode {
    /// Keep tables that pass the sanitizer, drop the rest (default).
    #[default]
    Sanitized,
    /// Keep every detected table, no questions asked.
    Raw,
    /// Never emit table markup; table text is re-flowed as paragraphs.
    Off,
}

/// Figure handling in markdown output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FigureMode {
    /// `![](img_id.png)` placeholders (default).
    #[default]
    Placeholder,
    /// Skip figures entirely.
    Skip,
    /// Keep the reference plus dimensions in metadata.
    Link,
}

/// Glyph corruption handling (`\u{FFFD}`, private-use and CJK-substituted
/// ligatures produced by broken PDF CMaps).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GlyphMode {
    /// Repair known substitutions and strip unmapped replacement characters.
    Repair,
    /// Only count them, leave text untouched (default).
    #[default]
    Report,
    /// Do not inspect glyphs at all.
    Off,
}

/// What to do when the extracted text looks like a failed glyph mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum JunkGuard {
    /// Record the hit in the sanitize report and metadata (default).
    #[default]
    Flag,
    /// Signal callers to re-parse with the fast backend.
    FallbackFast,
    /// Disabled.
    Off,
}

/// Failure policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OnError {
    /// Fail the whole load.
    #[default]
    Fail,
    /// Fail the page, keep going.
    PageError,
}

/// OCR mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OcrMode {
    /// Never OCR (default).
    #[default]
    Off,
    /// OCR only pages the backend flags as needing it.
    Auto,
    /// OCR every page.
    Always,
    /// Use an external OCR HTTP server.
    Server,
}

/// Heading emission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LevelMode {
    /// Keep backend levels; collapse to `h2` when every heading shares one level.
    #[default]
    Auto,
    /// Keep backend levels untouched.
    AsIs,
    /// Force `h2` for every heading.
    Flat,
    /// Derive levels by ranking heading block heights.
    Derive,
}

/// What to do with a table the backend returned without a header row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HeaderMissing {
    /// Reject it and route the cells through `demote_to` (default).
    #[default]
    Demote,
    /// Promote the first data row to the header and keep the table.
    Synthesize,
    /// Keep the headerless table as-is.
    Keep,
}

/// Where table text goes when a table is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DemoteTo {
    /// Re-flow the cells as a paragraph (default).
    #[default]
    Paragraph,
    /// Emit the cells as list items.
    List,
    /// Discard the block.
    Drop,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TableSanitize {
    /// Reject tables whose header row is missing or empty.
    pub require_header: bool,
    /// Reject tables whose median cell holds more than this many words.
    pub max_words_per_cell: usize,
    /// Reject tables with fewer than this many data rows.
    pub min_rows: usize,
    /// Reject tables with fewer than this many columns.
    pub min_columns: usize,
    /// Reject tables whose prose-looking cell ratio reaches this value.
    pub prose_cell_ratio: f32,
    /// Handling for tables the backend returned without a header row.
    pub header_missing: HeaderMissing,
    /// What to do with a rejected table.
    pub demote_to: DemoteTo,
}

impl Default for TableSanitize {
    fn default() -> Self {
        Self {
            require_header: true,
            max_words_per_cell: 3,
            min_rows: 1,
            min_columns: 2,
            prose_cell_ratio: 0.33,
            header_missing: HeaderMissing::Demote,
            demote_to: DemoteTo::Paragraph,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HeadingSanitize {
    /// Maximum heading length; longer blocks are demoted to paragraphs.
    pub max_len: usize,
    /// Demote headings that look like a truncated first line of a paragraph.
    pub drop_truncated: bool,
    /// How heading levels are assigned.
    pub levels: LevelMode,
}

impl Default for HeadingSanitize {
    fn default() -> Self {
        Self {
            max_len: 80,
            drop_truncated: true,
            levels: LevelMode::Auto,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SanitizeConfig {
    /// Master switch. `false` keeps backend output verbatim.
    pub enabled: bool,
    pub tables: TableSanitize,
    pub headings: HeadingSanitize,
    pub glyphs: GlyphMode,
    pub junk_guard: JunkGuard,
}

impl Default for SanitizeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            tables: TableSanitize::default(),
            headings: HeadingSanitize::default(),
            glyphs: GlyphMode::Report,
            junk_guard: JunkGuard::Flag,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExtractConfig {
    pub headings: bool,
    pub tables: TableMode,
    pub lists: bool,
    pub links: bool,
    pub figures: FigureMode,
    pub images: bool,
    pub form_fields: bool,
    pub annotations: bool,
    pub structure_tree: bool,
    pub vector_graphics: bool,
    pub text_metadata: bool,
    pub screenshots: bool,
}

impl Default for ExtractConfig {
    fn default() -> Self {
        Self {
            headings: true,
            tables: TableMode::Sanitized,
            lists: true,
            links: true,
            figures: FigureMode::Placeholder,
            images: false,
            form_fields: false,
            annotations: false,
            structure_tree: false,
            vector_graphics: false,
            text_metadata: false,
            screenshots: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OcrConfig {
    pub mode: OcrMode,
    pub language: String,
    pub server_url: Option<String>,
    pub headers: Vec<(String, String)>,
    pub tessdata_path: Option<String>,
    pub num_workers: usize,
    /// Fail the load when a requested OCR pass fails.
    pub failure_fatal: bool,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            mode: OcrMode::Off,
            language: "eng".to_string(),
            server_url: None,
            headers: Vec::new(),
            tessdata_path: None,
            num_workers: 0,
            failure_fatal: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScopeConfig {
    /// Page cap. `None` means every page (backsends often default to 1000).
    pub max_pages: Option<usize>,
    /// Page spec such as `"1-5,10"`.
    pub target_pages: Option<String>,
    pub password: Option<String>,
    /// Render DPI used for OCR and screenshots.
    pub dpi: f32,
    pub preserve_very_small_text: bool,
    pub skip_diagonal_text: bool,
    pub keep_headers_footers: bool,
    /// Emit progress logs.
    pub quiet: bool,
}

impl Default for ScopeConfig {
    fn default() -> Self {
        Self {
            max_pages: None,
            target_pages: None,
            password: None,
            dpi: 150.0,
            preserve_very_small_text: false,
            skip_diagonal_text: false,
            keep_headers_footers: false,
            quiet: true,
        }
    }
}

/// Complete parser configuration. Every field is user-selectable; the defaults
/// are the ones validated against this repo's PDF corpus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ParserConfig {
    pub backend: Backend,
    pub output: ParserOutput,
    pub granularity: Granularity,
    /// Attach the typed block list to `metadata["blocks"]`.
    pub keep_blocks: bool,
    /// Attach per-block bounding boxes.
    pub include_bboxes: bool,
    pub include_page_labels: bool,
    pub include_complexity: bool,
    pub extract: ExtractConfig,
    pub sanitize: SanitizeConfig,
    pub ocr: OcrConfig,
    pub scope: ScopeConfig,
    pub on_error: OnError,
    /// Stream parse output in page batches instead of one shot.
    pub streaming: bool,
}

impl Default for ParserConfig {
    fn default() -> Self {
        Self {
            backend: Backend::Auto,
            output: ParserOutput::Text,
            granularity: Granularity::Page,
            keep_blocks: true,
            include_bboxes: true,
            include_page_labels: true,
            include_complexity: false,
            extract: ExtractConfig::default(),
            sanitize: SanitizeConfig::default(),
            ocr: OcrConfig::default(),
            scope: ScopeConfig::default(),
            on_error: OnError::Fail,
            streaming: false,
        }
    }
}

impl ParserConfig {
    /// Named presets. Unknown names return an error listing valid ones.
    pub fn preset(name: &str) -> Result<Self, String> {
        match name {
            // Everything the backend produced, nothing rewritten.
            "faithful" => Ok(Self {
                output: ParserOutput::Markdown,
                keep_blocks: true,
                sanitize: SanitizeConfig {
                    enabled: false,
                    ..Default::default()
                },
                extract: ExtractConfig {
                    tables: TableMode::Raw,
                    headings: true,
                    ..Default::default()
                },
                ..Default::default()
            }),
            // Tuned for retrieval: sanitized structure, page bodies.
            "retrieval" => Ok(Self::default()),
            // One document per layout block, hierarchy kept.
            "structure" => Ok(Self {
                output: ParserOutput::Markdown,
                granularity: Granularity::Block,
                include_complexity: true,
                extract: ExtractConfig {
                    tables: TableMode::Raw,
                    ..Default::default()
                },
                ..Default::default()
            }),
            other => Err(format!(
                "unknown preset {other:?} (expected one of: faithful, retrieval, structure)"
            )),
        }
    }

    /// Parse a preset name or a JSON object of overrides.
    pub fn from_json_or_preset(spec: Option<&str>) -> Result<Self, String> {
        match spec {
            None => Ok(Self::default()),
            Some(s) => {
                let trimmed = s.trim();
                if trimmed.starts_with('{') {
                    serde_json::from_str(trimmed).map_err(|e| e.to_string())
                } else {
                    Self::preset(trimmed)
                }
            }
        }
    }
}
