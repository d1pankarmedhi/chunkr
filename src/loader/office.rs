//! Office Open XML and OpenDocument extraction: DOCX, PPTX and ODT.
//!
//! These formats are ZIP containers holding part-level XML. Text is pulled
//! straight from the relevant parts, so no rendering or intermediate model is
//! needed. DOCX and ODT yield one [`Document`]; PPTX yields one per slide.

use std::io::{Cursor, Read};

use quick_xml::events::Event;
use serde_json::Value;

use crate::error::ChunkrError;
use crate::loader::base::source_metadata;
use crate::loader::text::{clean_text, parse_error};
use crate::structures::document::Document;

/// Extracts text from DOCX, PPTX and ODT documents.
#[derive(Debug, Clone, Default)]
pub struct OfficeLoader {}

/// Container flavour decides which XML elements terminate lines and cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Markup {
    Docx,
    Pptx,
    Odt,
}

impl OfficeLoader {
    pub fn new() -> Self {
        Self {}
    }

    /// Load a DOCX, PPTX or ODT file into documents.
    pub fn load_from_file<P: AsRef<std::path::Path>>(
        &self,
        path: P,
        extension: &str,
    ) -> Result<Vec<Document>, ChunkrError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)?;
        self.load_documents(&bytes, extension, Some(&path.to_string_lossy()))
    }

    /// Load a DOCX, PPTX or ODT payload held in memory.
    pub fn load_documents(
        &self,
        bytes: &[u8],
        extension: &str,
        source: Option<&str>,
    ) -> Result<Vec<Document>, ChunkrError> {
        match extension.to_ascii_lowercase().as_str() {
            "docx" | "docm" => self.docx(bytes, source),
            "pptx" => self.pptx(bytes, source),
            "odt" => self.odt(bytes, source),
            other => Err(ChunkrError::ParseError(format!(
                "unsupported office format: .{other}"
            ))),
        }
    }

    fn docx(&self, bytes: &[u8], source: Option<&str>) -> Result<Vec<Document>, ChunkrError> {
        let xml = zip_entry(bytes, "word/document.xml")
            .ok_or_else(|| ChunkrError::ParseError("docx: missing word/document.xml".into()))?;
        let text = extract_markup(&xml, Markup::Docx)?;
        Ok(build_documents(text, source, "docx", |_| {}))
    }

    fn odt(&self, bytes: &[u8], source: Option<&str>) -> Result<Vec<Document>, ChunkrError> {
        let xml = zip_entry(bytes, "content.xml")
            .ok_or_else(|| ChunkrError::ParseError("odt: missing content.xml".into()))?;
        let text = extract_markup(&xml, Markup::Odt)?;
        Ok(build_documents(text, source, "odt", |_| {}))
    }

    fn pptx(&self, bytes: &[u8], source: Option<&str>) -> Result<Vec<Document>, ChunkrError> {
        let slides = zip_entries(bytes, "ppt/slides/slide", ".xml");
        if slides.is_empty() {
            return Err(ChunkrError::ParseError("pptx: no slides found".into()));
        }

        let total_slides = slides.len();
        let mut documents = Vec::with_capacity(total_slides);
        for (slide_number, (_, xml)) in slides.iter().enumerate() {
            let text = extract_markup(xml, Markup::Pptx)?;
            if text.is_empty() {
                continue;
            }
            documents.push(make_document(text, source, "pptx", |metadata| {
                metadata.insert("slide_number".to_string(), Value::from(slide_number + 1));
                metadata.insert("total_slides".to_string(), Value::from(total_slides));
            }));
        }

        Ok(documents)
    }
}

fn build_documents(
    text: String,
    source: Option<&str>,
    format: &str,
    extra: impl FnOnce(&mut std::collections::HashMap<String, Value>),
) -> Vec<Document> {
    if text.is_empty() {
        return Vec::new();
    }
    vec![make_document(text, source, format, extra)]
}

fn make_document(
    content: String,
    source: Option<&str>,
    format: &str,
    extra: impl FnOnce(&mut std::collections::HashMap<String, Value>),
) -> Document {
    let mut metadata = source_metadata(source);
    metadata.insert("format".to_string(), Value::from(format));
    extra(&mut metadata);
    Document::new(content, metadata)
}

/// Walk an OOXML / ODF part and collect text with structural line breaks.
fn extract_markup(xml: &str, markup: Markup) -> Result<String, ChunkrError> {
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut out = String::with_capacity(xml.len() / 2);

    loop {
        match reader.read_event().map_err(parse_error)? {
            Event::Text(text) => out.push_str(&text.xml10_content()),
            Event::CData(data) => out.push_str(&data.into_inner()),
            Event::Start(element) | Event::Empty(element) => {
                match (markup, element.name().as_ref()) {
                    (Markup::Docx, "w:tab") | (Markup::Pptx, "a:tab") => out.push('\t'),
                    (Markup::Docx, "w:br") | (Markup::Docx, "w:cr") | (Markup::Pptx, "a:br") => {
                        out.push('\n')
                    }
                    _ => {}
                }
            }
            Event::End(element) => match (markup, element.name().as_ref()) {
                (Markup::Docx, "w:p") | (Markup::Docx, "w:tr") => newline(&mut out),
                (Markup::Docx, "w:tc") => cell_separator(&mut out),
                (Markup::Pptx, "a:p") => newline(&mut out),
                (Markup::Odt, "text:p") | (Markup::Odt, "text:h") => newline(&mut out),
                (Markup::Odt, "table:table-cell") => cell_separator(&mut out),
                (Markup::Odt, "table:table-row") => newline(&mut out),
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(clean_text(&out))
}

fn newline(out: &mut String) {
    if !out.ends_with('\n') {
        out.push('\n');
    }
}

/// Close a table cell: drop the paragraph break so the row stays on one line.
fn cell_separator(out: &mut String) {
    while out.ends_with(['\n', ' ', '\t']) {
        out.pop();
    }
    if !out.is_empty() {
        out.push_str(" | ");
    }
}

/// Read a single archive entry as UTF-8 text.
fn zip_entry(bytes: &[u8], name: &str) -> Option<String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    let mut entry = archive.by_name(name).ok()?;
    let mut buffer = Vec::new();
    entry.read_to_end(&mut buffer).ok()?;
    Some(String::from_utf8_lossy(&buffer).into_owned())
}

/// Read every entry matching `prefix…suffix`, ordered by the number in the name.
fn zip_entries(bytes: &[u8], prefix: &str, suffix: &str) -> Vec<(String, String)> {
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes)) else {
        return Vec::new();
    };

    let mut names: Vec<String> = archive
        .file_names()
        .filter_map(Result::ok)
        .filter(|name| name.starts_with(prefix) && name.ends_with(suffix))
        .map(|name| name.into_owned())
        .collect();
    names.sort_by_key(|name| numeric_suffix(name));

    let mut entries = Vec::with_capacity(names.len());
    for name in names {
        let Ok(mut entry) = archive.by_name(&name) else {
            continue;
        };
        let mut buffer = Vec::new();
        if entry.read_to_end(&mut buffer).is_ok() {
            entries.push((name, String::from_utf8_lossy(&buffer).into_owned()));
        }
    }

    entries
}

/// Trailing integer of a part name, so `slide10.xml` sorts after `slide9.xml`.
fn numeric_suffix(name: &str) -> u32 {
    let start = name
        .rfind(|ch: char| !ch.is_ascii_digit())
        .map_or(0, |i| i + 1);
    name[start..].parse().unwrap_or(0)
}
