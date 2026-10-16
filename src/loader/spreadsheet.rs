//! Spreadsheet extraction for XLSX, XLSM, XLSB, XLS and ODS workbooks.
//!
//! Every worksheet becomes its own [`Document`] holding a Markdown table, so
//! sheet identity survives into chunk metadata and table chunking keeps headers.

use std::io::Cursor;
use std::path::Path;

use calamine::{open_workbook_auto_from_rs, Data, Range, Reader};
use serde_json::Value;

use crate::error::ChunkrError;
use crate::loader::base::source_metadata;
use crate::loader::table_source::render_markdown_table;
use crate::loader::text::parse_error;
use crate::structures::document::Document;

/// Extracts worksheet contents from spreadsheet workbooks.
#[derive(Debug, Clone, Default)]
pub struct SpreadsheetLoader {}

impl SpreadsheetLoader {
    pub fn new() -> Self {
        Self {}
    }

    /// Load every sheet of a workbook file, in workbook order.
    pub fn load_from_file<P: AsRef<Path>>(&self, path: P) -> Result<String, ChunkrError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)?;
        let documents = self.load_documents(&bytes, Some(&path.to_string_lossy()))?;
        Ok(join_documents(&documents))
    }

    /// Load every sheet of a workbook held in memory.
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

        for (sheet_index, sheet_name) in sheet_names.iter().enumerate() {
            let range = workbook
                .worksheet_range(sheet_name)
                .map_err(|e| parse_error(format!("spreadsheet sheet '{sheet_name}': {e}")))?;
            let (row_count, column_count) = range.get_size();
            let content = range_to_markdown(&range);
            if content.is_empty() {
                continue;
            }

            let mut metadata = source_metadata(source);
            metadata.insert("format".to_string(), Value::from("spreadsheet"));
            metadata.insert("sheet_name".to_string(), Value::from(sheet_name.clone()));
            metadata.insert("sheet_index".to_string(), Value::from(sheet_index));
            metadata.insert("total_sheets".to_string(), Value::from(total_sheets));
            metadata.insert("row_count".to_string(), Value::from(row_count));
            metadata.insert("column_count".to_string(), Value::from(column_count));
            documents.push(Document::new(content, metadata));
        }

        Ok(documents)
    }
}

/// Render a worksheet range as a Markdown table with a header row.
fn range_to_markdown(range: &Range<Data>) -> String {
    let mut rows = range.rows();
    let Some(header) = rows.next() else {
        return String::new();
    };

    let header: Vec<String> = header.iter().map(cell_text).collect();
    let body: Vec<Vec<String>> = rows
        .map(|row| row.iter().map(cell_text).collect())
        .collect();

    render_markdown_table(&header, &body)
}

/// Cell value as plain text; escaping happens in [`render_markdown_table`].
fn cell_text(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        other => other.to_string(),
    }
}

fn join_documents(documents: &[Document]) -> String {
    documents
        .iter()
        .map(|doc| doc.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}
