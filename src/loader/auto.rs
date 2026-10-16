//! Automatic format dispatch: pick the right extractor from a file extension.
//!
//! Extensions without a dedicated parser are decoded as text: byte-order marks are
//! always honoured and `.txt`/`.log`/`.csv`/`.tsv` also accept legacy single-byte
//! encodings, while formats that are UTF-8 by convention are rejected when they are
//! not valid UTF-8.

use std::path::Path;

use serde_json::Value;

use crate::error::ChunkrError;
use crate::loader::base::source_metadata;
use crate::loader::email::EmailLoader;
use crate::loader::epub::EpubLoader;
use crate::loader::office::OfficeLoader;
use crate::loader::pdf::PDFLoader;
use crate::loader::spreadsheet::SpreadsheetLoader;
use crate::loader::subtitle::SubtitleLoader;
use crate::loader::text::{
    clean_text, decode_text, looks_binary, notebook_to_text, rtf_to_text, xml_to_text,
};
use crate::loader::web::html_to_text;
use crate::structures::document::Document;

/// Formats with a dedicated extractor rather than a plain-text fallback.
pub const STRUCTURED_EXTENSIONS: &[&str] = &[
    "pdf", "xlsx", "xlsm", "xlsb", "xls", "ods", "docx", "docm", "pptx", "odt", "epub", "eml",
    "mbox", "rtf", "xml", "ipynb", "html", "htm", "xhtml", "srt", "vtt", "webvtt",
];

/// Extensions understood by [`load_documents`], structured formats first.
pub fn supported_extensions() -> Vec<&'static str> {
    let mut extensions = STRUCTURED_EXTENSIONS.to_vec();
    extensions.extend_from_slice(&[
        "txt", "md", "markdown", "csv", "tsv", "json", "jsonl", "ndjson", "yaml", "yml",
    ]);
    extensions
}

/// True when the extension has a dedicated extractor.
pub fn is_structured(extension: &str) -> bool {
    let extension = normalize(extension);
    STRUCTURED_EXTENSIONS.contains(&extension.as_str())
}

/// Extract documents from a file, choosing the extractor from its extension.
pub fn load_documents<P: AsRef<Path>>(path: P) -> Result<Vec<Document>, ChunkrError> {
    let path = path.as_ref();
    let source = path.to_string_lossy().to_string();
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default();
    let bytes = std::fs::read(path)?;
    load_documents_from_bytes(&bytes, extension, Some(&source))
}

/// Text formats where a legacy single-byte or UTF-16 encoding is plausible
/// enough to decode rather than reject. Other text formats are UTF-8 by
/// convention, so a payload that is not valid UTF-8 is reported instead of
/// silently turned into mojibake.
const LEGACY_ENCODING_FORMATS: &[&str] = &["txt", "text", "log", "csv", "tsv"];

/// Extract documents from an in-memory payload with a known extension.
pub fn load_documents_from_bytes(
    bytes: &[u8],
    extension: &str,
    source: Option<&str>,
) -> Result<Vec<Document>, ChunkrError> {
    let documents = extract_documents(bytes, &normalize(extension), source)
        .map_err(|error| qualify_source(source, error))?;

    Ok(documents
        .into_iter()
        .filter(|doc| !doc.content.trim().is_empty())
        .collect())
}

/// Extension dispatch. Errors carry no source context, so callers can add their own.
fn extract_documents(
    bytes: &[u8],
    extension: &str,
    source: Option<&str>,
) -> Result<Vec<Document>, ChunkrError> {
    let documents = match extension {
        "pdf" => PDFLoader::new().load_pages_from_bytes(bytes)?,
        "xlsx" | "xlsm" | "xlsb" | "xls" | "ods" => {
            SpreadsheetLoader::new().load_documents(bytes, source)?
        }
        "docx" | "docm" | "pptx" | "odt" => {
            OfficeLoader::new().load_documents(bytes, extension, source)?
        }
        "epub" => EpubLoader::new().load_documents(bytes, source)?,
        "srt" | "vtt" | "webvtt" => SubtitleLoader::new().load_documents(bytes, extension, source)?,
        "eml" | "mbox" | "mail" => EmailLoader::new().load_documents(bytes, extension, source)?,
        "html" | "htm" | "xhtml" => {
            vec![document(clean_text(&html_to_text(bytes)?), source, "html")]
        }
        "rtf" => vec![document(rtf_to_text(&decode_text(bytes)), source, "rtf")],
        "xml" => vec![document(xml_to_text(&decode_text(bytes))?, source, "xml")],
        "ipynb" => vec![document(
            notebook_to_text(&decode_text(bytes))?,
            source,
            "ipynb",
        )],
        _ => {
            let has_bom = encoding_rs::Encoding::for_bom(bytes).is_some();
            if !has_bom && looks_binary(bytes) {
                return Err(ChunkrError::ParseError(format!(
                    "binary payload is not a supported format: .{extension}"
                )));
            }
            let content = if has_bom || LEGACY_ENCODING_FORMATS.contains(&extension) {
                decode_text(bytes)
            } else {
                utf8_text(bytes, extension)?
            };
            // JSON is a family of its own even though it decodes as text.
            let family = if matches!(extension, "json" | "jsonl" | "ndjson") {
                "json"
            } else {
                "text"
            };
            vec![document(content, source, family)]
        }
    };

    Ok(documents)
}

/// Decode a payload whose format is UTF-8 by convention.
fn utf8_text(bytes: &[u8], extension: &str) -> Result<String, ChunkrError> {
    std::str::from_utf8(bytes).map(str::to_string).map_err(|_| {
        let label = if extension.is_empty() {
            "text"
        } else {
            extension
        };
        ChunkrError::ParseError(format!(
            "{label} payload is not valid UTF-8 (legacy encodings are decoded for .txt, .log, .csv, .tsv)"
        ))
    })
}

/// Prefix extraction errors with the payload source, so batch callers can name
/// the file that failed.
fn qualify_source(source: Option<&str>, error: ChunkrError) -> ChunkrError {
    match (source, error) {
        (Some(source), ChunkrError::ParseError(message)) => {
            ChunkrError::ParseError(format!("{source}: {message}"))
        }
        (_, error) => error,
    }
}

fn normalize(extension: &str) -> String {
    extension
        .trim_start_matches('.')
        .trim()
        .to_ascii_lowercase()
}

fn document(content: String, source: Option<&str>, format: &str) -> Document {
    let mut metadata = source_metadata(source);
    metadata.insert("format".to_string(), Value::from(format));
    Document::new(content, metadata)
}
