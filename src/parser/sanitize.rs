//! Heuristic cleanup of backend layout blocks.
//!
//! Every rule here is configurable and every change is reported, so callers can
//! run with `sanitize.enabled = false` and get backend output untouched.

use serde::{Deserialize, Serialize};

use super::config::{DemoteTo, GlyphMode, HeadingSanitize, JunkGuard, LevelMode, SanitizeConfig};
use super::payload::{median_words_per_cell, BlockKind, BlockPayload, PagePayload};

/// Ligature and private-use substitutions produced by broken PDF CMaps.
/// Observed on re-encoded textbook PDFs (a glyph substituted for `i` inside a name).
const GLYPH_REPAIRS: &[(char, &str)] = &[
    ('\u{FB00}', "ff"),
    ('\u{FB01}', "fi"),
    ('\u{FB02}', "fl"),
    ('\u{FB03}', "ffi"),
    ('\u{FB04}', "ffl"),
    ('\u{FA00}', "f"),  // observed: 昀
    ('\u{6900}', "i"),  // observed: 椀
    ('\u{6800}', "j"),  // observed: 樀
    ('\u{CF00}', "fi"), // observed: 케
];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SanitizeReport {
    pub tables_kept: usize,
    pub tables_demoted: usize,
    pub headings_kept: usize,
    pub headings_demoted: usize,
    pub glyph_repairs: usize,
    pub junk_hits: usize,
    /// Set when the caller should retry with the fast backend.
    pub fallback_fast: bool,
    pub notes: Vec<String>,
}

impl SanitizeReport {
    fn note(&mut self, note: String) {
        if self.notes.len() < 32 {
            self.notes.push(note);
        }
    }

    /// True when nothing was rewritten.
    pub fn is_clean(&self) -> bool {
        self.tables_demoted == 0
            && self.headings_demoted == 0
            && self.glyph_repairs == 0
            && self.junk_hits == 0
    }
}

/// Sanitize every page in place and return a report of what changed.
pub fn sanitize_pages(pages: &mut [PagePayload], cfg: &SanitizeConfig) -> SanitizeReport {
    let mut report = SanitizeReport::default();
    if !cfg.enabled {
        return report;
    }

    let flat_levels = matches!(cfg.headings.levels, LevelMode::Flat)
        || (matches!(cfg.headings.levels, LevelMode::Auto) && single_level_headings(pages));

    for page in pages.iter_mut() {
        if !matches!(cfg.glyphs, GlyphMode::Off) {
            repair_glyphs(page, cfg.glyphs, &mut report);
        }
        if !matches!(cfg.junk_guard, JunkGuard::Off) {
            guard_junk(page, cfg.junk_guard, &mut report);
        }
        sanitize_blocks(
            &mut page.blocks,
            &cfg.tables,
            &cfg.headings,
            flat_levels,
            &mut report,
        );
    }

    if matches!(cfg.headings.levels, LevelMode::Derive) {
        derive_heading_levels(pages, &mut report);
    }
    report
}

fn single_level_headings(pages: &[PagePayload]) -> bool {
    let mut levels = pages
        .iter()
        .flat_map(|p| p.blocks.iter())
        .filter(|b| b.kind() == BlockKind::Heading)
        .map(|b| b.level.unwrap_or(0));
    match levels.next() {
        None => false,
        Some(first) => levels.all(|l| l == first),
    }
}

fn sanitize_blocks(
    blocks: &mut Vec<BlockPayload>,
    tables: &super::config::TableSanitize,
    headings: &HeadingSanitize,
    flat_levels: bool,
    report: &mut SanitizeReport,
) {
    // Rebuild the list so rejected tables can be re-flowed into paragraphs.
    let mut out: Vec<BlockPayload> = Vec::with_capacity(blocks.len());
    for idx in 0..blocks.len() {
        let mut block = blocks[idx].clone();

        match block.kind() {
            BlockKind::Table => {
                if let Some(reason) = evaluate_table(&mut block, tables) {
                    report.tables_demoted += 1;
                    report.note(format!("{reason} (block {idx})"));
                    match tables.demote_to {
                        DemoteTo::Drop => continue,
                        DemoteTo::List => out.push(demote_to_list(block)),
                        DemoteTo::Paragraph => out.push(demote_to_paragraph(block)),
                    }
                    continue;
                }
                report.tables_kept += 1;
                out.push(block);
            }
            BlockKind::Heading => {
                let text = block.text.clone().unwrap_or_default();
                let next_is_lowercase_para = blocks[idx + 1..]
                    .iter()
                    .find(|b| !b.text_content().trim().is_empty())
                    .map(|b| {
                        b.kind() == BlockKind::Paragraph
                            && b.text
                                .as_deref()
                                .and_then(|t| t.trim_start().chars().next())
                                .map(|c| c.is_lowercase())
                                .unwrap_or(false)
                    })
                    .unwrap_or(false);
                let too_long = text.chars().count() > headings.max_len;
                let truncated =
                    headings.drop_truncated && next_is_lowercase_para && ends_mid_sentence(&text);
                if too_long || truncated {
                    report.headings_demoted += 1;
                    report.note(format!(
                        "heading demoted ({}) : {:?}",
                        if too_long {
                            "too long"
                        } else {
                            "truncated lead-in"
                        },
                        truncate(&text, 60)
                    ));
                    out.push(demote_to_paragraph(block));
                    continue;
                }
                if flat_levels {
                    block.level = Some(2);
                }
                report.headings_kept += 1;
                out.push(block);
            }
            _ => out.push(block),
        }
    }
    *blocks = out;
}

fn evaluate_table(block: &mut BlockPayload, cfg: &super::config::TableSanitize) -> Option<String> {
    if !block.has_header() {
        match cfg.header_missing {
            super::config::HeaderMissing::Demote => {
                return Some("table has no header row".to_string())
            }
            super::config::HeaderMissing::Synthesize => {
                if let Some(rows) = block.rows.as_mut() {
                    if rows.len() > 1 {
                        let first = rows.remove(0);
                        block.header = Some(first);
                    }
                }
            }
            super::config::HeaderMissing::Keep => {}
        }
        if cfg.require_header && !block.has_header() {
            return Some("table has no header row".to_string());
        }
    }

    let rows = block.table_rows();
    let data_rows = rows.len().saturating_sub(usize::from(block.has_header()));
    let max_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);

    if data_rows < cfg.min_rows {
        return Some(format!("table has {data_rows} data rows"));
    }
    if max_cols < cfg.min_columns {
        return Some(format!("table has {max_cols} columns"));
    }
    let median = median_words_per_cell(block);
    if median > cfg.max_words_per_cell as f32 {
        return Some(format!("median {median} words per cell looks re-flowed"));
    }
    let cells: Vec<String> = rows
        .iter()
        .flatten()
        .filter(|c| !c.is_empty())
        .cloned()
        .collect();
    if !cells.is_empty() {
        let prose = cells.iter().filter(|c| looks_prose(c)).count();
        let ratio = prose as f32 / cells.len() as f32;
        if ratio >= cfg.prose_cell_ratio {
            return Some(format!(
                "{prose}/{} cells look like prose ({ratio:.2})",
                cells.len()
            ));
        }
    }
    None
}

/// A cell looks like re-flowed prose rather than a value.
fn looks_prose(cell: &str) -> bool {
    let words = cell.split_whitespace().count();
    let has_sentence = cell.ends_with('.') || cell.ends_with(',');
    let numeric = cell.chars().filter(|c| c.is_ascii_digit()).count();
    words >= 3 && (has_sentence || numeric * 4 < cell.chars().count())
}

fn ends_mid_sentence(text: &str) -> bool {
    match text.trim_end().chars().last() {
        Some(c) => !matches!(c, '.' | ':' | '?' | '!' | ';'),
        None => false,
    }
}

fn demote_to_paragraph(block: BlockPayload) -> BlockPayload {
    // Rejected tables re-flow into a single line so wrapped sentences rejoin.
    let text = if block.kind() == BlockKind::Table {
        block.table_text_flat()
    } else {
        block.text_content()
    };
    BlockPayload {
        kind: "paragraph".to_string(),
        text: Some(text),
        level: None,
        header: None,
        rows: None,
        lines: None,
        marker: None,
        ordered: None,
        ..block
    }
}

fn demote_to_list(block: BlockPayload) -> BlockPayload {
    let text = block.text_content();
    BlockPayload {
        kind: "list_item".to_string(),
        text: Some(text),
        ordered: Some(false),
        marker: Some("-".to_string()),
        header: None,
        rows: None,
        lines: None,
        level: None,
        ..block
    }
}

fn repair_glyphs(page: &mut PagePayload, mode: GlyphMode, report: &mut SanitizeReport) {
    let mut hits = 0usize;
    let fix = |s: &mut String, hits: &mut usize| {
        let mut out = String::with_capacity(s.len());
        for ch in s.chars() {
            if let Some((_, replacement)) = GLYPH_REPAIRS.iter().find(|(from, _)| *from == ch) {
                *hits += 1;
                if matches!(mode, GlyphMode::Repair) {
                    out.push_str(replacement);
                } else {
                    out.push(ch);
                }
            } else if ch == '\u{FFFD}' {
                *hits += 1;
                if matches!(mode, GlyphMode::Repair) {
                    // Unmapped replacement character carries no information.
                } else {
                    out.push(ch);
                }
            } else {
                out.push(ch);
            }
        }
        if matches!(mode, GlyphMode::Repair) {
            *s = out;
        }
    };

    fix(&mut page.text, &mut hits);
    if let Some(md) = page.markdown.as_mut() {
        fix(md, &mut hits);
    }
    for block in page.blocks.iter_mut() {
        if let Some(text) = block.text.as_mut() {
            fix(text, &mut hits);
        }
        if let Some(lines) = block.lines.as_mut() {
            for line in lines.iter_mut() {
                fix(line, &mut hits);
            }
        }
        for cell in block.table_cells_mut() {
            fix(&mut cell.text, &mut hits);
        }
    }
    if hits > 0 {
        report.glyph_repairs += hits;
        report.note(format!(
            "{hits} corrupted glyph(s) on page {}",
            page.page_number
        ));
    }
}

fn guard_junk(page: &PagePayload, mode: JunkGuard, report: &mut SanitizeReport) {
    let hits = ["Unimplemented", "Identity-H", "Identity-V"]
        .iter()
        .map(|marker| page.text.matches(marker).count())
        .sum::<usize>();
    if hits == 0 {
        return;
    }
    report.junk_hits += hits;
    report.note(format!(
        "{hits} unmapped-glyph marker(s) on page {}",
        page.page_number
    ));
    if matches!(mode, JunkGuard::FallbackFast) {
        report.fallback_fast = true;
    }
}

/// Rank heading blocks by height and map the distinct heights onto levels 1..=6.
fn derive_heading_levels(pages: &mut [PagePayload], report: &mut SanitizeReport) {
    let mut heights: Vec<i32> = pages
        .iter()
        .flat_map(|p| p.blocks.iter())
        .filter(|b| b.kind() == BlockKind::Heading)
        .filter_map(|b| b.bbox.map(|b| (b.height * 2.0).round() as i32))
        .collect();
    heights.sort_unstable();
    heights.dedup();
    heights.reverse();
    if heights.len() < 2 {
        report.note("heading levels: fewer than two distinct heading sizes".to_string());
        return;
    }
    for page in pages.iter_mut() {
        for block in page.blocks.iter_mut() {
            if block.kind() != BlockKind::Heading {
                continue;
            }
            if let Some(bbox) = block.bbox {
                let key = (bbox.height * 2.0).round() as i32;
                if let Some(rank) = heights.iter().position(|h| *h == key) {
                    block.level = Some((rank as u8 + 1).min(6));
                }
            }
        }
    }
}

fn truncate(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}
