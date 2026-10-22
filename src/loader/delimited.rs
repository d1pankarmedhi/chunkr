//! Delimited text (`.csv`, `.tsv`) loaded as tables.
//!
//! A CSV export is a table, not prose: loading it as one Markdown table document
//! keeps headers attached to every row, lets a chunk keep its columns, and makes
//! the output identical to what the spreadsheet loader and the extension table
//! sources produce. Parsing is dependency-free and quote-aware — embedded
//! delimiters and newlines stay inside a cell, `""` unescapes to a quote, and
//! CRLF line endings are accepted.

use crate::error::ChunkrError;
use crate::loader::table_source::{documents_from_tables, TableData};
use crate::loader::text::decode_text;
use crate::structures::document::Document;

/// Load a delimited payload as one table document.
///
/// One `Document` per file: a delimited file has no sheets to split on, and row
/// windows are a chunking decision the chunker makes later with `row_start`.
pub fn load_delimited(
    bytes: &[u8],
    extension: &str,
    source: Option<&str>,
) -> Result<Vec<Document>, ChunkrError> {
    let delimiter = if extension == "tsv" { b'\t' } else { b',' };
    let text = decode_text(bytes);
    let mut records = parse_records(&text, delimiter).into_iter();
    let header = records.next().unwrap_or_default();
    let rows: Vec<Vec<String>> = records
        .filter(|row| row.iter().any(|cell| !cell.trim().is_empty()))
        .collect();

    if header.is_empty() {
        return Err(ChunkrError::EmptyInput);
    }

    Ok(documents_from_tables(
        vec![TableData {
            name: None,
            columns: header,
            rows,
            row_offset: Some(0),
        }],
        extension,
        source,
    ))
}

/// Split delimited text into records of fields.
///
/// Quotes are respected, not required: an unquoted cell that happens to contain
/// quotes is kept as written, so hand-edited exports load instead of failing.
pub fn parse_records(text: &str, delimiter: u8) -> Vec<Vec<String>> {
    let bytes = text.as_bytes();
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut index = 0;

    while index < bytes.len() {
        let byte = bytes[index];
        if quoted {
            if byte == b'"' {
                if bytes.get(index + 1) == Some(&b'"') {
                    field.push('"');
                    index += 2;
                    continue;
                }
                quoted = false;
            } else {
                push_char(&mut field, bytes, &mut index);
                continue;
            }
            index += 1;
            continue;
        }

        match byte {
            b'"' if field.trim().is_empty() => {
                field.clear();
                quoted = true;
                index += 1;
            }
            b'\r' => index += 1,
            b'\n' => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
                index += 1;
            }
            _ if byte == delimiter => {
                record.push(std::mem::take(&mut field));
                index += 1;
            }
            _ => push_char(&mut field, bytes, &mut index),
        }
    }

    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    records
}

/// Append one UTF-8 character, advancing `index` past it.
fn push_char(field: &mut String, bytes: &[u8], index: &mut usize) {
    let rest = &bytes[*index..];
    let width = utf8_width(rest.first().copied().unwrap_or(b'\0'));
    let end = (*index + width).min(bytes.len());
    field.push_str(&String::from_utf8_lossy(&bytes[*index..end]));
    *index = end;
}

fn utf8_width(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}
