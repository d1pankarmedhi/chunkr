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
    /// OCR only pages whose complexity signals say the text layer is broken.
    Auto,
    /// OCR every page.
    Always,
    /// OCR every page through an external OCR HTTP server.
    Server,
}

/// Which OCR engine a config resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrBackendKind {
    /// Built-in Tesseract.
    Tesseract,
    /// Built-in ONNX PP-OCR (needs the `pdf-ocr-ppocr` feature).
    Ppocr,
    /// HTTP server speaking the liteparse OCR API.
    Server,
    /// A Python-registered engine, forwarded through the plugin's loopback proxy.
    Custom(String),
}

/// Complexity reasons that make a page worth OCRing under `mode="auto"`.
///
/// Deliberately excludes `sparse_text`/`embedded_images`/`vector_text`: slides,
/// covers and image-heavy digital pages carry those signals while their text
/// layer is fine, so gating on them would OCR most pages of a normal report.
/// Add them through `ocr.auto_reasons` when a corpus needs it.
pub const DEFAULT_OCR_REASONS: [&str; 3] = ["scanned", "no_text", "garbled"];

/// Accelerators `oar-ocr` can be compiled with.
pub const PPOCR_DEVICES: [&str; 7] = [
    "cpu", "coreml", "cuda", "directml", "openvino", "tensorrt", "webgpu",
];

/// PP-OCR (ONNX) options, used by the `pdf-ocr-ppocr` feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PpocrConfig {
    /// Model trio: `tiny`, `small`, `medium`.
    pub preset: String,
    /// Model cache / download directory (`$OAR_HOME` when unset).
    pub models_dir: Option<String>,
    /// Accelerator the engine was compiled with.
    pub device: String,
}

impl Default for PpocrConfig {
    fn default() -> Self {
        Self {
            preset: "small".to_string(),
            models_dir: None,
            device: "cpu".to_string(),
        }
    }
}

/// Python-engine options, forwarded to the plugin's loopback OCR server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginOcrConfig {
    /// Engine-specific options (model name, base URL, API key env var, ...).
    pub options: serde_json::Map<String, serde_json::Value>,
    /// Timeout for one page image.
    pub timeout_ms: u64,
    /// Page images the engine serves at once; engines that are not thread-safe
    /// serialize behind a lock regardless of this value.
    pub concurrency: usize,
    /// Load the engine when the parser is built instead of on the first page.
    pub warmup: bool,
    /// Loopback port; `0` picks an ephemeral one.
    pub port: u16,
}

impl Default for PluginOcrConfig {
    fn default() -> Self {
        Self {
            options: serde_json::Map::new(),
            timeout_ms: 60_000,
            concurrency: 1,
            warmup: true,
            port: 0,
        }
    }
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
    /// `tesseract`, `ppocr`, `server`, or the name of a registered engine.
    pub backend: String,
    pub language: String,
    pub server_url: Option<String>,
    pub headers: Vec<(String, String)>,
    /// Send hedged duplicate requests after these delays (ms) and take the
    /// first reply; useful for slow remote OCR.
    pub hedge_delays_ms: Vec<u64>,
    pub tessdata_path: Option<String>,
    pub num_workers: usize,
    /// Fail the load when a requested OCR pass fails.
    pub failure_fatal: bool,
    /// Complexity reasons that trigger OCR under `mode="auto"`.
    /// Empty means [`DEFAULT_OCR_REASONS`].
    pub auto_reasons: Vec<String>,
    /// Minimum text length for a page to count as fine under `mode="auto"`.
    pub auto_min_chars: usize,
    pub ppocr: PpocrConfig,
    pub plugin: PluginOcrConfig,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            mode: OcrMode::Off,
            backend: "tesseract".to_string(),
            language: "eng".to_string(),
            server_url: None,
            headers: Vec::new(),
            hedge_delays_ms: Vec::new(),
            tessdata_path: None,
            num_workers: 0,
            failure_fatal: false,
            auto_reasons: Vec::new(),
            auto_min_chars: 0,
            ppocr: PpocrConfig::default(),
            plugin: PluginOcrConfig::default(),
        }
    }
}

impl OcrConfig {
    /// Which engine the config selects. `mode="server"`, or a `server_url` left
    /// with the default backend, both mean the HTTP engine.
    pub fn backend_kind(&self) -> OcrBackendKind {
        if matches!(self.mode, OcrMode::Server) {
            return OcrBackendKind::Server;
        }
        match self.backend.trim().to_ascii_lowercase().as_str() {
            "ppocr" | "pp-ocr" | "oar" | "oar-ocr" => OcrBackendKind::Ppocr,
            "server" | "http" | "http-server" => OcrBackendKind::Server,
            "" | "tesseract" if self.server_url.is_none() => OcrBackendKind::Tesseract,
            "" | "tesseract" => OcrBackendKind::Server,
            other => OcrBackendKind::Custom(other.to_string()),
        }
    }

    /// True when OCR runs at all.
    pub fn is_enabled(&self) -> bool {
        !matches!(self.mode, OcrMode::Off)
    }

    /// Reasons gating `mode="auto"`, normalized (`sparse-text` == `sparse_text`).
    pub fn reasons(&self) -> Vec<String> {
        let source: Vec<&str> = if self.auto_reasons.is_empty() {
            DEFAULT_OCR_REASONS.to_vec()
        } else {
            self.auto_reasons.iter().map(String::as_str).collect()
        };
        source
            .iter()
            .map(|reason| normalize_reason(reason))
            .collect()
    }

    /// Language in the form the selected engine expects.
    pub fn engine_language(&self) -> String {
        match self.backend_kind() {
            OcrBackendKind::Tesseract => ocr_language_tesseract(&self.language),
            _ => ocr_language_iso(&self.language),
        }
    }

    /// Whether one page needs OCR under `mode="auto"`.
    ///
    /// The reason list decides; the backend's `needs_ocr` flag is only used when
    /// a backend reports no reasons at all, because that flag is true for
    /// ordinary digital pages too.
    pub fn page_needs_ocr(&self, page: &crate::parser::PagePayload) -> bool {
        let text_length = if page.complexity.is_some() {
            page.complexity.as_ref().map(|c| c.text_length).unwrap_or(0)
        } else {
            page.text.chars().count()
        };
        let too_short = text_length < self.auto_min_chars;
        match page.complexity.as_ref() {
            Some(complexity) => {
                if complexity.reasons.is_empty() {
                    return complexity.needs_ocr || complexity.is_garbled || too_short;
                }
                let gated = self.reasons().iter().any(|wanted| {
                    complexity
                        .reasons
                        .iter()
                        .map(|reason| normalize_reason(reason))
                        .any(|reason| reason == *wanted)
                });
                gated || complexity.is_garbled || too_short
            }
            // No complexity signals: OCR the page rather than return an empty one.
            None => true,
        }
    }
}

/// `sparse-text` and `sparse_text` name the same signal.
fn normalize_reason(reason: &str) -> String {
    reason.trim().to_lowercase().replace('-', "_")
}

/// ISO 639-1 code for engines specified in it (HTTP OCR API, plugin registry).
pub fn ocr_language_iso(code: &str) -> String {
    let lowered = code.trim().to_ascii_lowercase();
    match lowered.as_str() {
        "eng" => "en",
        "chi_sim" | "chi" | "zho" | "zh_cn" => "zh",
        "chi_tra" | "zh_tw" => "zh-tw",
        "jpn" => "ja",
        "kor" => "ko",
        "fra" | "fre" => "fr",
        "deu" | "ger" => "de",
        "spa" => "es",
        "ita" => "it",
        "por" => "pt",
        "nld" | "dut" => "nl",
        "rus" => "ru",
        "ara" => "ar",
        "hin" => "hi",
        "tur" => "tr",
        "pol" => "pl",
        "ukr" => "uk",
        "vie" => "vi",
        "tha" => "th",
        "heb" => "he",
        "ell" | "gre" => "el",
        "ces" | "cze" => "cs",
        "swe" => "sv",
        "dan" => "da",
        "fin" => "fi",
        "nor" => "no",
        "hun" => "hu",
        "ron" | "rum" => "ro",
        "ind" => "id",
        "msa" | "may" => "ms",
        "tam" => "ta",
        "tel" => "te",
        "ben" => "bn",
        "urd" => "ur",
        other => other,
    }
    .to_string()
}

/// Tesseract code for a language named in any supported form.
pub fn ocr_language_tesseract(code: &str) -> String {
    let lowered = code.trim().to_ascii_lowercase();
    match lowered.as_str() {
        "en" => "eng",
        "zh" | "zh_cn" | "zh-hans" => "chi_sim",
        "zh-tw" | "zh_tw" | "zh-hant" => "chi_tra",
        "ja" => "jpn",
        "ko" => "kor",
        "fr" => "fra",
        "de" => "deu",
        "es" => "spa",
        "it" => "ita",
        "pt" => "por",
        "nl" => "nld",
        "ru" => "rus",
        "ar" => "ara",
        "hi" => "hin",
        "tr" => "tur",
        "pl" => "pol",
        "uk" => "ukr",
        "vi" => "vie",
        "th" => "tha",
        "he" => "heb",
        "el" => "ell",
        "cs" => "ces",
        "sv" => "swe",
        "da" => "dan",
        "fi" => "fin",
        "no" => "nor",
        "hu" => "hun",
        "ro" => "ron",
        "id" => "ind",
        "ms" => "msa",
        "ta" => "tam",
        "te" => "tel",
        "bn" => "ben",
        "ur" => "urd",
        other => other,
    }
    .to_string()
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
    /// Reject configurations that cannot work, with actionable messages.
    ///
    /// Backend availability (a Cargo feature, an installed plugin) is checked
    /// where the parse runs, not here, because it differs per language.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(spec) = self.scope.target_pages.as_deref() {
            let spec = spec.trim();
            if !spec.is_empty() {
                parse_page_spec(spec).map_err(|e| format!("invalid scope.target_pages: {e}"))?;
            }
        }
        if self.scope.dpi <= 0.0 {
            return Err(format!(
                "scope.dpi must be positive, got {}",
                self.scope.dpi
            ));
        }
        let ocr = &self.ocr;
        match ocr.backend_kind() {
            OcrBackendKind::Server if ocr.is_enabled() && ocr.server_url.is_none() => {
                return Err(
                    "ocr.backend=\"server\" needs ocr.server_url (masked to the liteparse OCR API)"
                        .to_string(),
                )
            }
            OcrBackendKind::Ppocr => {
                if !["tiny", "small", "medium"].contains(&ocr.ppocr.preset.as_str()) {
                    return Err(format!(
                        "ocr.ppocr.preset must be one of tiny, small, medium (got {:?})",
                        ocr.ppocr.preset
                    ));
                }
                let device = ocr.ppocr.device.trim().to_ascii_lowercase();
                if !PPOCR_DEVICES.contains(&device.as_str()) {
                    return Err(format!(
                        "ocr.ppocr.device must be one of {} (got {:?})",
                        PPOCR_DEVICES.join(", "),
                        ocr.ppocr.device
                    ));
                }
            }
            _ => {}
        }
        if ocr.is_enabled() && ocr.backend.trim().is_empty() {
            return Err(
                "ocr.backend must not be empty; use tesseract, ppocr, server, or a registered name"
                    .to_string(),
            );
        }
        if ocr.plugin.concurrency == 0 {
            return Err("ocr.plugin.concurrency must be at least 1".to_string());
        }
        if ocr.plugin.timeout_ms == 0 {
            return Err("ocr.plugin.timeout_ms must be positive".to_string());
        }
        Ok(())
    }

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
                let config: Self = if trimmed.starts_with('{') {
                    serde_json::from_str(trimmed).map_err(|e| e.to_string())?
                } else {
                    Self::preset(trimmed)?
                };
                config.validate()?;
                Ok(config)
            }
        }
    }
}

/// Parse a `"1-5,10"` page spec, mirroring the backend's parser.
pub(crate) fn parse_page_spec(spec: &str) -> Result<Vec<u32>, String> {
    #[cfg(all(feature = "pdf", not(target_arch = "wasm32")))]
    {
        liteparse::config::parse_target_pages(spec)
    }
    #[cfg(not(all(feature = "pdf", not(target_arch = "wasm32"))))]
    {
        let mut pages = Vec::new();
        for part in spec.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            match part.split_once('-') {
                Some((start, end)) => {
                    let start: u32 = start
                        .trim()
                        .parse()
                        .map_err(|_| format!("bad range {part:?}"))?;
                    let end: u32 = end
                        .trim()
                        .parse()
                        .map_err(|_| format!("bad range {part:?}"))?;
                    if start == 0 || end < start {
                        return Err(format!("bad range {part:?}"));
                    }
                    pages.extend(start..=end);
                }
                None => {
                    let page: u32 = part.parse().map_err(|_| format!("bad page {part:?}"))?;
                    if page == 0 {
                        return Err(format!("bad page {part:?}"));
                    }
                    pages.push(page);
                }
            }
        }
        if pages.is_empty() {
            return Err(format!("no pages in {spec:?}"));
        }
        pages.sort_unstable();
        pages.dedup();
        Ok(pages)
    }
}
