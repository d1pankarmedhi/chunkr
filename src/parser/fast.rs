//! Fast `lopdf` path: text only, no layout structure. Used as the fallback
//! backend and whenever a caller asks for maximum throughput.

use lopdf::Document as LopdfDoc;

use crate::error::ChunkrError;

use super::payload::PagePayload;

/// Extract one payload per page using `lopdf`'s text extraction.
pub fn pages_from_bytes(bytes: &[u8]) -> Result<Vec<PagePayload>, ChunkrError> {
    let doc = LopdfDoc::load_mem(bytes)?;
    let mut pages = Vec::new();
    for (index, &page_number) in doc.get_pages().keys().enumerate() {
        let text = doc.extract_text(&[page_number]).unwrap_or_default();
        pages.push(PagePayload {
            page_number: index + 1,
            text,
            ..Default::default()
        });
    }
    Ok(pages)
}
