//! Which pages OCR under `mode="auto"`, and how to name them for the backend.

use super::config::{parse_page_spec, ParserConfig};
use super::payload::PagePayload;

/// Pages the configured gate selects, ascending. Respects an explicit
/// `scope.target_pages` filter, so auto-OCR never widens a user's scope.
pub fn select_ocr_pages(pages: &[PagePayload], cfg: &ParserConfig) -> Vec<usize> {
    let scope = cfg
        .scope
        .target_pages
        .as_deref()
        .map(str::trim)
        .filter(|spec| !spec.is_empty())
        .and_then(|spec| parse_page_spec(spec).ok());

    let mut selected: Vec<usize> = pages
        .iter()
        .filter(|page| cfg.ocr.page_needs_ocr(page))
        .map(|page| page.page_number)
        .filter(|number| match &scope {
            Some(scope) => scope.contains(&(*number as u32)),
            None => true,
        })
        .collect();
    selected.sort_unstable();
    selected.dedup();
    selected
}

/// `"1,3-5"` for a page list; empty when there is nothing to OCR.
pub fn format_page_range(pages: &[usize]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut start: Option<usize> = None;
    let mut previous = 0usize;
    for page in pages.iter().copied() {
        match start {
            None => start = Some(page),
            Some(_) if page == previous + 1 => {}
            Some(first) => {
                parts.push(range_part(first, previous));
                start = Some(page);
            }
        }
        previous = page;
    }
    if let Some(first) = start {
        parts.push(range_part(first, previous));
    }
    parts.join(",")
}

fn range_part(first: usize, last: usize) -> String {
    if first == last {
        first.to_string()
    } else {
        format!("{first}-{last}")
    }
}
