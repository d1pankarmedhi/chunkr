//! Spreadsheet extraction for XLSX, XLSM, XLSB, XLS and ODS workbooks.
//!
//! Every worksheet becomes its own [`Document`] holding a Markdown table, so
//! sheet identity survives into chunk metadata and table chunking keeps headers.
//!
//! Worksheets are read structurally rather than naively: merged regions are
//! filled so a category spanning rows labels each of them, a title row above the
//! header becomes `sheet_title`, vertically merged headers become multi-level
//! headers (`2024 / Q1`), dates and durations are rendered as text instead of
//! Excel serial numbers, percentages and currency use their display format, and
//! a cell whose cached value is missing falls back to its formula.

use std::io::Cursor;
use std::path::Path;

use calamine::{
    open_workbook_auto_from_rs, Data, Dimensions, ExcelDateTime, Range, Reader, Sheets,
};

use serde_json::Value;

use crate::error::ChunkrError;
use crate::loader::base::source_metadata;
use crate::loader::spreadsheet_format::SheetFormats;
use crate::loader::table_source::render_markdown_table;
use crate::loader::text::parse_error;
use crate::structures::document::Document;

/// A multi-level header deeper than this is not a header; something else is
/// going on, so the extra rows stay in the body.
const MAX_HEADER_ROWS: u32 = 4;

/// Extracts worksheet contents from spreadsheet workbooks.
#[derive(Debug, Clone, Default)]
pub struct SpreadsheetLoader {}

impl SpreadsheetLoader {
    pub fn new() -> Self {
        Self {}
    }

    pub fn load_from_file<P: AsRef<Path>>(&self, path: P) -> Result<String, ChunkrError> {
        let bytes = std::fs::read(path)?;
        self.load_from_bytes(&bytes)
    }

    pub fn load_from_bytes(&self, bytes: &[u8]) -> Result<String, ChunkrError> {
        let documents = self.load_documents(bytes, None)?;
        Ok(join_documents(&documents))
    }

    /// Load a workbook into one `Document` per non-empty worksheet.
    pub fn load_documents(
        &self,
        bytes: &[u8],
        source: Option<&str>,
    ) -> Result<Vec<Document>, ChunkrError> {
        let mut workbook = open_workbook_auto_from_rs(Cursor::new(bytes.to_vec()))
            .map_err(|e| parse_error(format!("spreadsheet: {e}")))?;
        let sheet_names = workbook.sheet_names();
        let total_sheets = sheet_names.len();
        let mut documents = Vec::with_capacity(total_sheets);
        // Number formats live in the xlsx parts calamine does not surface.
        let styled = matches!(workbook, Sheets::Xlsx(_));

        for (sheet_index, sheet_name) in sheet_names.iter().enumerate() {
            let range = workbook
                .worksheet_range(sheet_name)
                .map_err(|e| parse_error(format!("spreadsheet sheet '{sheet_name}': {e}")))?;
            let merges = merged_regions(&mut workbook, sheet_name);
            let formulas = formulas_for(&mut workbook, sheet_name, &range);
            let formats = styled
                .then(|| SheetFormats::read(bytes, sheet_name))
                .flatten();

            let table = SheetTable::build(&range, &merges, formulas.as_ref(), formats.as_ref());
            let content = table.to_markdown();
            if content.is_empty() {
                continue;
            }

            let mut metadata = source_metadata(source);
            metadata.insert("format".to_string(), Value::from("spreadsheet"));
            metadata.insert("sheet_name".to_string(), Value::from(sheet_name.clone()));
            metadata.insert("sheet_index".to_string(), Value::from(sheet_index));
            metadata.insert("total_sheets".to_string(), Value::from(total_sheets));
            metadata.insert("row_count".to_string(), Value::from(table.body_rows()));
            metadata.insert("column_count".to_string(), Value::from(table.header.len()));
            if let Some(title) = table.title {
                metadata.insert("sheet_title".to_string(), Value::from(title));
            }
            if table.header_rows > 1 {
                metadata.insert("header_rows".to_string(), Value::from(table.header_rows));
            }
            documents.push(Document::new(content, metadata));
        }

        Ok(documents)
    }
}

/// Merged regions for a sheet, for the formats that report them.
///
/// XLSB and ODS expose no merged regions; their sheets are read as-is.
fn merged_regions(workbook: &mut Sheets<Cursor<Vec<u8>>>, sheet_name: &str) -> Vec<Dimensions> {
    match workbook {
        Sheets::Xlsx(inner) => inner
            .merge_cells_by_sheet_name(sheet_name)
            .unwrap_or_default(),
        Sheets::Xls(inner) => inner
            .merge_cells_by_sheet_name(sheet_name)
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Formula text per cell, when asking for it is worth a second sheet pass.
///
/// Formula text only matters where a cached value is missing, which needs a
/// sheet with empty cells; dense sheets skip the pass entirely, so the cost
/// lands on the sparse sheets where it is small.
fn formulas_for(
    workbook: &mut Sheets<Cursor<Vec<u8>>>,
    sheet_name: &str,
    range: &Range<Data>,
) -> Option<Range<String>> {
    if range
        .rows()
        .all(|row| row.iter().all(|cell| !matches!(cell, Data::Empty)))
    {
        return None;
    }
    workbook
        .worksheet_formula(sheet_name)
        .ok()
        .filter(|formulas| !formulas.is_empty())
}

/// A worksheet resolved into title, header and body cells.
struct SheetTable {
    title: Option<String>,
    header: Vec<String>,
    header_rows: u32,
    rows: Vec<Vec<String>>,
}

impl SheetTable {
    fn build(
        range: &Range<Data>,
        merges: &[Dimensions],
        formulas: Option<&Range<String>>,
        formats: Option<&SheetFormats>,
    ) -> Self {
        // A range need not start at A1, so grid coordinates are relative to
        // where the sheet's used area begins while merges and styles are not.
        let offset = range.start().unwrap_or((0, 0));
        let mut grid = value_grid(range, formulas, formats);
        fill_merged_regions(&mut grid, merges, offset);
        let grid = trim_empty_columns(grid);

        let mut start = first_content_row(&grid);
        let title = match start {
            Some(row) if is_title_row(&grid, row) => {
                let title = single_value(&grid[row as usize]);
                start = next_content_row(&grid, row + 1);
                title
            }
            _ => None,
        };

        let Some(header_row) = start else {
            return Self {
                title,
                header: Vec::new(),
                header_rows: 1,
                rows: Vec::new(),
            };
        };

        let header_rows = header_height(&grid, header_row, merges, offset);
        let body_start = (header_row + header_rows).min(grid.len() as u32);
        let header = join_header_rows(&grid, header_row, header_rows);

        Self {
            title,
            header,
            header_rows,
            rows: grid[body_start as usize..].to_vec(),
        }
    }

    fn to_markdown(&self) -> String {
        render_markdown_table(&self.header, &self.rows)
    }

    /// Body rows, excluding blank ones the renderer drops.
    fn body_rows(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| row.iter().any(|cell| !cell.is_empty()))
            .count()
    }
}

/// Cell values as text, one row per used row, formulas filling empty cells.
///
/// A sheet's used area need not start at A1, and the formula range has an offset
/// of its own, so every lookup goes through absolute positions.
fn value_grid(
    range: &Range<Data>,
    formulas: Option<&Range<String>>,
    formats: Option<&SheetFormats>,
) -> Vec<Vec<String>> {
    let (height, width) = range.get_size();
    let offset = range.start().unwrap_or((0, 0));
    let mut grid = Vec::with_capacity(height);

    for row in 0..height as u32 {
        let mut cells = Vec::with_capacity(width);
        for column in 0..width as u32 {
            let position = (offset.0 + row, offset.1 + column);
            let cell = range.get_value(position).unwrap_or(&Data::Empty);
            let formula = formulas
                .and_then(|formulas| formulas.get_value(position))
                .map(String::as_str);
            cells.push(match cell {
                Data::Float(value) => formats
                    .and_then(|formats| formats.render(position.0, position.1, *value))
                    .unwrap_or_else(|| cell_text(cell, formula)),
                _ => cell_text(cell, formula),
            });
        }
        grid.push(cells);
    }

    grid
}

/// Copy the value of a merged region into every cell it covers.
fn fill_merged_regions(grid: &mut [Vec<String>], merges: &[Dimensions], offset: (u32, u32)) {
    for region in merges {
        let Some((start_row, start_column)) = relative(region.start, offset) else {
            continue;
        };
        let Some((end_row, end_column)) = relative(region.end, offset) else {
            continue;
        };
        let Some(value) = grid
            .get(start_row as usize)
            .and_then(|row| row.get(start_column as usize))
            .cloned()
        else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        for row in start_row..=end_row.min(grid.len() as u32 - 1) {
            for column in start_column..=end_column {
                if let Some(cell) = grid[row as usize].get_mut(column as usize) {
                    if cell.is_empty() {
                        *cell = value.clone();
                    }
                }
            }
        }
    }
}

/// Absolute cell position to grid position, when the cell is inside the range.
fn relative(position: (u32, u32), offset: (u32, u32)) -> Option<(u32, u32)> {
    Some((
        position.0.checked_sub(offset.0)?,
        position.1.checked_sub(offset.1)?,
    ))
}

/// Drop trailing columns with no content anywhere, which are usually formatting.
fn trim_empty_columns(mut grid: Vec<Vec<String>>) -> Vec<Vec<String>> {
    let width = grid.iter().map(Vec::len).max().unwrap_or(0);
    let keep = (0..width)
        .rev()
        .find(|&column| {
            grid.iter()
                .any(|row| row.get(column).is_some_and(|cell| !cell.is_empty()))
        })
        .map(|column| column + 1)
        .unwrap_or(0);
    for row in grid.iter_mut() {
        row.truncate(keep);
    }
    grid
}

fn first_content_row(grid: &[Vec<String>]) -> Option<u32> {
    next_content_row(grid, 0)
}

fn next_content_row(grid: &[Vec<String>], from: u32) -> Option<u32> {
    (from..grid.len() as u32).find(|&row| has_content(&grid[row as usize]))
}

fn has_content(row: &[String]) -> bool {
    row.iter().any(|cell| !cell.is_empty())
}

/// A row holding one distinct value while the next content row holds several is
/// a title above the header — whether it is a single cell or merged across the
/// table, since filling a merge repeats its value.
fn is_title_row(grid: &[Vec<String>], row: u32) -> bool {
    let distinct = distinct_values(&grid[row as usize]);
    if distinct != 1 {
        return false;
    }
    next_content_row(grid, row + 1)
        .map(|next| distinct_values(&grid[next as usize]) > 1)
        .unwrap_or(false)
}

fn distinct_values(row: &[String]) -> usize {
    let mut seen: Vec<&str> = Vec::new();
    for cell in row.iter().filter(|cell| !cell.is_empty()) {
        if !seen.contains(&cell.as_str()) {
            seen.push(cell);
        }
    }
    seen.len()
}

fn single_value(row: &[String]) -> Option<String> {
    row.iter()
        .find(|cell| !cell.is_empty())
        .filter(|_| distinct_values(row) == 1)
        .cloned()
}

/// How many rows the header occupies: one, plus any rows a merge started in the
/// header spans into.
fn header_height(
    grid: &[Vec<String>],
    header_row: u32,
    merges: &[Dimensions],
    offset: (u32, u32),
) -> u32 {
    let spanned = merges
        .iter()
        .filter(|region| relative(region.start, offset).map(|(row, _)| row) == Some(header_row))
        .map(|region| region.end.0 - region.start.0 + 1)
        .max()
        .unwrap_or(1)
        .min(MAX_HEADER_ROWS)
        .min(grid.len() as u32 - header_row);

    spanned.max(1)
}

/// Join a multi-row header into one label per column (`2024 / Q1`).
fn join_header_rows(grid: &[Vec<String>], header_row: u32, header_rows: u32) -> Vec<String> {
    let width = grid
        .get(header_row as usize)
        .map(Vec::len)
        .unwrap_or_default();
    (0..width)
        .map(|column| {
            let mut levels: Vec<&str> = Vec::new();
            for row in header_row..header_row + header_rows {
                let Some(cell) = grid.get(row as usize).and_then(|row| row.get(column)) else {
                    continue;
                };
                if cell.is_empty() || levels.contains(&cell.as_str()) {
                    continue;
                }
                levels.push(cell);
            }
            levels.join(" / ")
        })
        .collect()
}

/// Cell value as plain text; escaping happens in [`render_markdown_table`].
fn cell_text(cell: &Data, formula: Option<&str>) -> String {
    match cell {
        Data::Empty => formula
            .map(|formula| format!("={formula}"))
            .unwrap_or_default(),
        Data::DateTime(value) => excel_datetime_text(value),
        other => other.to_string(),
    }
}

/// Excel stores dates and times as serial numbers; render them as text.
fn excel_datetime_text(value: &ExcelDateTime) -> String {
    if value.is_duration() {
        let total_seconds = (value.as_f64() * 86_400.0).round().max(0.0) as u64;
        return format!(
            "{:02}:{:02}:{:02}",
            total_seconds / 3_600,
            (total_seconds % 3_600) / 60,
            total_seconds % 60
        );
    }

    let (year, month, day, hour, minute, second, _millis) = value.to_ymd_hms_milli();
    if hour == 0 && minute == 0 && second == 0 {
        format!("{year:04}-{month:02}-{day:02}")
    } else {
        format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
    }
}

fn join_documents(documents: &[Document]) -> String {
    documents
        .iter()
        .map(|doc| doc.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}
