//! HTML extraction: converts markup into readable text before chunking.

use crate::error::ChunkrError;

/// html2text wraps lines at this width; large enough to keep paragraphs intact
/// so the chunker, not the renderer, decides the boundaries.
const TEXT_WIDTH: usize = 40_000;

/// Convert HTML, XHTML or MHTML-style markup into readable plain text.
pub fn html_to_text(html: &[u8]) -> Result<String, ChunkrError> {
    html2text::from_read(html, TEXT_WIDTH).map_err(crate::loader::text::parse_error)
}
