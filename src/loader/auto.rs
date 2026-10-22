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
use crate::loader::msg::MsgLoader;
use crate::loader::office::OfficeLoader;
use crate::loader::pdf::PDFLoader;
use crate::loader::spreadsheet::SpreadsheetLoader;
use crate::loader::subtitle::SubtitleLoader;
use crate::loader::table_source::{
    documents_from_tables, has_table_source, registered_extensions, table_source_for, TableSource,
};
use crate::loader::text::{
    clean_text, decode_text, looks_binary, notebook_to_text, rtf_to_text, xml_to_text,
};
use crate::loader::web::html_to_text;
use crate::structures::document::Document;

/// Formats with a dedicated extractor rather than a plain-text fallback.
pub const STRUCTURED_EXTENSIONS: &[&str] = &[
    "pdf", "xlsx", "xlsm", "xlsb", "xls", "ods", "docx", "docm", "pptx", "odt", "epub", "eml",
    "mbox", "msg", "rtf", "xml", "ipynb", "html", "htm", "xhtml", "srt", "vtt", "webvtt", "zip",
];

/// Extensions understood by [`load_documents`], structured formats first.
///
/// Includes every extension claimed by a registered [`TableSource`], so an
/// installed extension package widens this list without touching core.
pub fn supported_extensions() -> Vec<String> {
    let mut extensions: Vec<String> = STRUCTURED_EXTENSIONS
        .iter()
        .map(|e| e.to_string())
        .collect();
    extensions.extend(
        [
            "txt", "md", "markdown", "csv", "tsv", "json", "jsonl", "ndjson", "yaml", "yml",
        ]
        .iter()
        .map(|e| e.to_string()),
    );
    extensions.extend(
        audio_extensions()
            .iter()
            .map(|extension| extension.to_string()),
    );
    extensions.extend(registered_extensions());
    extensions
}

/// Extensions this build can decode as audio (empty without the `audio` feature).
///
/// Containers that need an external decoder (`opus`, `webm`, `mp4`, `mov`, …) are
/// listed only when `CHUNKR_FFMPEG` is set: `supported_extensions()` is a promise,
/// so it reflects what this process can actually open.
pub fn audio_extensions() -> &'static [&'static str] {
    #[cfg(all(feature = "audio", not(target_arch = "wasm32")))]
    {
        if crate::audio::ffmpeg_configured() {
            crate::audio::all_audio_extensions()
        } else {
            crate::audio::AUDIO_EXTENSIONS
        }
    }
    #[cfg(not(all(feature = "audio", not(target_arch = "wasm32"))))]
    {
        &[]
    }
}

/// True when the extension is an audio container this build decodes.
pub fn is_audio(extension: &str) -> bool {
    let extension = normalize(extension);
    audio_extensions().contains(&extension.as_str())
}

/// True when the extension has a dedicated extractor.
pub fn is_structured(extension: &str) -> bool {
    let extension = normalize(extension);
    STRUCTURED_EXTENSIONS.contains(&extension.as_str())
        || has_table_source(&extension)
        || is_audio(&extension)
}

/// Lower-case extension of a path or archive member name, without the dot.
pub(crate) fn extension_of(name: &str) -> String {
    normalize(
        Path::new(name)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default(),
    )
}

/// Extract documents from a file, choosing the extractor from its extension.
pub fn load_documents<P: AsRef<Path>>(path: P) -> Result<Vec<Document>, ChunkrError> {
    let path = path.as_ref();
    let source = path.to_string_lossy().to_string();
    let extension = normalize(
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default(),
    );

    // A registered source streams from the file instead of the payload being
    // read into memory first.
    if let Some(table_source) = table_source_for(&extension) {
        return load_from_table_source(
            table_source.as_ref(),
            Some(path),
            None,
            &extension,
            Some(&source),
        )
        .map_err(|error| qualify_source(Some(&source), error));
    }

    let bytes = std::fs::read(path)?;
    load_documents_from_bytes(&bytes, &extension, Some(&source))
}

/// Extract documents using a registered table source.
///
/// The source owns extraction (it may stream from `path`); the mapping to
/// documents — Markdown rendering, escaping and span metadata — stays here, so
/// extension output matches the built-in spreadsheet loader.
fn load_from_table_source(
    table_source: &dyn TableSource,
    path: Option<&Path>,
    bytes: Option<&[u8]>,
    extension: &str,
    source: Option<&str>,
) -> Result<Vec<Document>, ChunkrError> {
    // Callers qualify errors with the source: this path is reached from both
    // `load_documents` and `load_documents_from_bytes`.
    let tables = table_source.load(path, bytes, extension, source)?;

    Ok(documents_from_tables(tables, extension, source)
        .into_iter()
        .filter(|document| !document.content.trim().is_empty())
        .collect())
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
    extract_payload(bytes, &normalize(extension), source, 0)
}

/// Extract a payload, tracking how many archives already contain it.
pub(crate) fn extract_payload(
    bytes: &[u8],
    extension: &str,
    source: Option<&str>,
    depth: usize,
) -> Result<Vec<Document>, ChunkrError> {
    let documents = extract_documents(bytes, extension, source, depth)
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
    depth: usize,
) -> Result<Vec<Document>, ChunkrError> {
    // Audio decodes and transcribes rather than extracting text; the arm lives
    // behind the feature that provides the decoder.
    #[cfg(all(feature = "audio", not(target_arch = "wasm32")))]
    if is_audio(extension) {
        return crate::audio::load_documents(bytes, extension, source, None);
    }

    let documents = match extension {
        "zip" => {
            return crate::loader::archive::expand_zip(
                bytes,
                source,
                &crate::loader::archive::ArchiveLimits::from_env(),
                depth,
            )
        }
        "pdf" => PDFLoader::new().load_pages_from_bytes(bytes)?,
        "xlsx" | "xlsm" | "xlsb" | "xls" | "ods" => {
            SpreadsheetLoader::new().load_documents(bytes, source)?
        }
        "docx" | "docm" | "pptx" | "odt" => {
            OfficeLoader::new().load_documents(bytes, extension, source)?
        }
        "epub" => EpubLoader::new().load_documents(bytes, source)?,
        "srt" | "vtt" | "webvtt" => {
            SubtitleLoader::new().load_documents(bytes, extension, source)?
        }
        "eml" | "mbox" | "mail" => EmailLoader::new().load_documents(bytes, extension, source)?,
        "msg" => MsgLoader::new().load_documents(bytes, source)?,
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
            if let Some(table_source) = table_source_for(extension) {
                return load_from_table_source(
                    table_source.as_ref(),
                    None,
                    Some(bytes),
                    extension,
                    source,
                );
            }
            // Nothing to go on from the extension: let the payload name itself.
            // Known text extensions are authoritative and never overridden.
            if !supported_extensions()
                .iter()
                .any(|known| known == extension)
            {
                if let Some(sniffed) = sniff_extension(bytes) {
                    return extract_documents(bytes, sniffed, source, depth);
                }
            }
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
            // JSONL is machine-written one-record-per-line data: a broken line is
            // corruption, so it is reported rather than silently text-chunked.
            if matches!(extension, "jsonl" | "ndjson") {
                validate_json_lines(&content)?;
            }
            vec![document(content, source, family)]
        }
    };

    Ok(documents)
}

/// Identify a payload whose extension is missing or unrecognised.
///
/// Deliberately dependency-free: the formats this crate supports are either
/// magic-prefixed (PDF, RTF) or zip containers whose entries name them
/// (docx/xlsx/pptx/odt/epub). Anything else stays unknown and falls through to
/// the text or binary handling — never guessed.
fn sniff_extension(bytes: &[u8]) -> Option<&'static str> {
    let prefix = |magic: &[u8]| bytes.starts_with(magic);
    if prefix(b"%PDF-") {
        return Some("pdf");
    }
    if prefix(b"{\\rtf") {
        return Some("rtf");
    }
    if !prefix(b"PK\x03\x04") && !prefix(b"PK\x05\x06") {
        return None;
    }

    let archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let names: Vec<String> = archive
        .file_names()
        .filter_map(Result::ok)
        .map(String::from)
        .collect();
    let has = |name: &str| names.iter().any(|entry| entry == name);

    if has("mimetype") || has("META-INF/container.xml") {
        return Some("epub");
    }
    if has("word/document.xml") {
        return Some("docx");
    }
    if has("xl/workbook.xml") {
        return Some("xlsx");
    }
    if has("ppt/presentation.xml") || names.iter().any(|entry| entry.starts_with("ppt/slides/")) {
        return Some("pptx");
    }
    if has("content.xml") && has("META-INF/manifest.xml") {
        return Some("odt");
    }
    // Any other valid zip archive.
    Some("zip")
}

/// Reject a JSONL/NDJSON payload whose lines are not all JSON values.
fn validate_json_lines(text: &str) -> Result<(), ChunkrError> {
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Err(error) = serde_json::from_str::<Value>(line) {
            return Err(ChunkrError::ParseError(format!(
                "line {}: {error}",
                index + 1
            )));
        }
    }
    Ok(())
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
