//! EPUB extraction: spine-ordered chapters converted to readable text.

use std::io::{Cursor, Read};

use quick_xml::events::{BytesStart, Event};
use serde_json::Value;

use crate::error::ChunkrError;
use crate::loader::base::source_metadata;
use crate::loader::text::{clean_text, parse_error};
use crate::loader::web::html_to_text;
use crate::structures::document::Document;

/// Extracts chapter text from EPUB archives.
#[derive(Debug, Clone, Default)]
pub struct EpubLoader {}

#[derive(Debug, Default)]
struct Package {
    title: Option<String>,
    creator: Option<String>,
    /// Manifest id -> archive path.
    manifest: Vec<(String, String)>,
    /// Spine item ids in reading order.
    spine: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum Capture {
    Title,
    Creator,
    None,
}

impl EpubLoader {
    pub fn new() -> Self {
        Self {}
    }

    /// Load an EPUB file into one document per spine chapter.
    pub fn load_from_file<P: AsRef<std::path::Path>>(
        &self,
        path: P,
    ) -> Result<Vec<Document>, ChunkrError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)?;
        self.load_documents(&bytes, Some(&path.to_string_lossy()))
    }

    /// Load an EPUB payload held in memory.
    pub fn load_documents(
        &self,
        bytes: &[u8],
        source: Option<&str>,
    ) -> Result<Vec<Document>, ChunkrError> {
        let opf_path = package_path(bytes)?;
        let opf = read_entry(bytes, &opf_path)
            .ok_or_else(|| ChunkrError::ParseError(format!("epub: missing {opf_path}")))?;
        let package = parse_package(&opf, &opf_path)?;

        let mut documents = Vec::new();
        for id in &package.spine {
            let Some((_, href)) = package.manifest.iter().find(|(item_id, _)| item_id == id) else {
                continue;
            };
            if !is_document(href) {
                continue;
            }
            let Some(chapter) = read_entry(bytes, href) else {
                continue;
            };
            let text = html_to_text(chapter.as_bytes())?;
            let text = clean_text(&text);
            if text.is_empty() {
                continue;
            }

            let mut metadata = source_metadata(source);
            metadata.insert("format".to_string(), Value::from("epub"));
            metadata.insert("chapter_index".to_string(), Value::from(documents.len()));
            metadata.insert("chapter_href".to_string(), Value::from(href.clone()));
            if let Some(title) = package.title.clone() {
                metadata.insert("book_title".to_string(), Value::from(title));
            }
            if let Some(creator) = package.creator.clone() {
                metadata.insert("book_author".to_string(), Value::from(creator));
            }
            documents.push(Document::new(text, metadata));
        }

        if documents.is_empty() {
            return Err(ChunkrError::ParseError(
                "epub: no readable spine chapters".into(),
            ));
        }

        Ok(documents)
    }
}

/// Locate the OPF package document through `META-INF/container.xml`.
fn package_path(bytes: &[u8]) -> Result<String, ChunkrError> {
    let container = read_entry(bytes, "META-INF/container.xml")
        .ok_or_else(|| ChunkrError::ParseError("epub: missing META-INF/container.xml".into()))?;

    let mut reader = quick_xml::Reader::from_str(&container);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event().map_err(parse_error)? {
            Event::Start(element) | Event::Empty(element) => {
                if element.name().as_ref() == "rootfile" {
                    if let Some(path) = attribute(&element, "full-path")? {
                        return Ok(path);
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    Err(ChunkrError::ParseError(
        "epub: container.xml has no rootfile".into(),
    ))
}

/// Parse manifest, spine and Dublin Core metadata from the OPF document.
fn parse_package(opf: &str, opf_path: &str) -> Result<Package, ChunkrError> {
    let mut reader = quick_xml::Reader::from_str(opf);
    reader.config_mut().trim_text(true);
    let mut package = Package::default();
    let mut capture = Capture::None;

    loop {
        match reader.read_event().map_err(parse_error)? {
            Event::Start(element) | Event::Empty(element) => match element.name().as_ref() {
                "item" => {
                    if let (Some(id), Some(href)) =
                        (attribute(&element, "id")?, attribute(&element, "href")?)
                    {
                        package.manifest.push((id, resolve_href(opf_path, &href)));
                    }
                }
                "itemref" => {
                    if let Some(idref) = attribute(&element, "idref")? {
                        package.spine.push(idref);
                    }
                }
                "dc:title" => capture = Capture::Title,
                "dc:creator" => capture = Capture::Creator,
                _ => {}
            },
            Event::Text(text) => {
                let value = text.xml10_content().trim().to_string();
                match capture {
                    Capture::Title if !value.is_empty() => package.title = Some(value),
                    Capture::Creator if !value.is_empty() => package.creator = Some(value),
                    _ => {}
                }
            }
            Event::End(element) => match element.name().as_ref() {
                "dc:title" | "dc:creator" => capture = Capture::None,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(package)
}

fn attribute(element: &BytesStart<'_>, key: &str) -> Result<Option<String>, ChunkrError> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(parse_error)?;
        if attribute.key.as_ref() == key {
            let value = attribute
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map_err(parse_error)?;
            return Ok(Some(value.into_owned()));
        }
    }
    Ok(None)
}

/// Resolve a manifest href against the directory holding the OPF document.
fn resolve_href(opf_path: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or(href);
    if href.starts_with('/') {
        return href.trim_start_matches('/').to_string();
    }

    let base = opf_path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    let mut parts: Vec<&str> = base.split('/').filter(|part| !part.is_empty()).collect();
    for segment in href.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn is_document(href: &str) -> bool {
    let lower = href.to_ascii_lowercase();
    lower.ends_with(".xhtml") || lower.ends_with(".html") || lower.ends_with(".htm")
}

/// Read an archive entry, tolerating percent-encoded manifest hrefs.
fn read_entry(bytes: &[u8], name: &str) -> Option<String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    let decoded = percent_decode(name);
    let mut candidates = vec![name];
    if decoded != name {
        candidates.push(&decoded);
    }

    let mut buffer = Vec::new();
    for candidate in candidates {
        if let Ok(mut entry) = archive.by_name(candidate) {
            entry.read_to_end(&mut buffer).ok()?;
            return Some(String::from_utf8_lossy(&buffer).into_owned());
        }
    }

    None
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
