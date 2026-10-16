//! Registry for optional tabular extractors (Parquet, Arrow IPC, ORC, ...).
//!
//! The core crate stays free of columnar dependencies: an extension package
//! extracts a table (or a batch of rows) and hands it over as plain strings,
//! while this module owns the mapping to [`Document`]s — Markdown rendering,
//! escaping, metadata and span keys — so extension output is indistinguishable
//! from the built-in spreadsheet loader.
//!
//! Both Rust crates and Python plugins can register a source: the Python bridge
//! wraps a callable into a [`TableSource`] (see `crate::python`), a Rust
//! extension implements the trait directly.

use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard};

use serde_json::Value;

use crate::error::ChunkrError;
use crate::loader::base::source_metadata;
use crate::structures::document::Document;

/// One extracted table: a header plus row-major cell text.
///
/// Cells are unescaped: Markdown escaping and blank-row handling happen here in
/// the core, not in the extractor.
#[derive(Debug, Clone, Default)]
pub struct TableData {
    /// Table/row-group name the source uses (rendered as `table_name`).
    pub name: Option<String>,
    /// Column names; empty cells are named `column_<n>` when rendered.
    pub columns: Vec<String>,
    /// Row-major cell text, already stringified by the extractor.
    pub rows: Vec<Vec<String>>,
    /// 0-based index of this table's first row inside the source, when known.
    pub row_offset: Option<u64>,
}

/// An optional tabular extractor, registered by name.
pub trait TableSource: Send + Sync {
    /// Extensions this source claims, lower-case and without the dot.
    fn extensions(&self) -> Vec<String>;

    /// Extract tables from a file or an in-memory payload.
    ///
    /// Exactly one of `path`/`bytes` is set: a path lets the source stream from
    /// disk, bytes are what `load_documents_from_bytes` already holds.
    fn load(
        &self,
        path: Option<&Path>,
        bytes: Option<&[u8]>,
        extension: &str,
        source: Option<&str>,
    ) -> Result<Vec<TableData>, ChunkrError>;
}

type Registry = RwLock<Vec<(String, Arc<dyn TableSource>)>>;

fn registry() -> &'static Registry {
    static SOURCES: OnceLock<Registry> = OnceLock::new();
    SOURCES.get_or_init(|| RwLock::new(Vec::new()))
}

// A poisoned lock cannot happen (no code panics while holding it) and would not
// invalidate the registrations anyway, so recover the guard instead of failing.
fn read_registry() -> RwLockReadGuard<'static, Vec<(String, Arc<dyn TableSource>)>> {
    registry().read().unwrap_or_else(|error| error.into_inner())
}

fn write_registry() -> RwLockWriteGuard<'static, Vec<(String, Arc<dyn TableSource>)>> {
    registry()
        .write()
        .unwrap_or_else(|error| error.into_inner())
}

/// Register a table source under `name`; registering the same name replaces it.
pub fn register_table_source(name: &str, source: Box<dyn TableSource>) {
    let mut sources = write_registry();
    sources.retain(|(existing, _)| existing != name);
    sources.push((name.to_string(), Arc::from(source)));
}

/// Remove a registered source. Returns `true` when one was removed.
pub fn unregister_table_source(name: &str) -> bool {
    let mut sources = write_registry();
    let before = sources.len();
    sources.retain(|(existing, _)| existing != name);
    sources.len() != before
}

/// Names of every registered source, in registration order.
pub fn table_sources() -> Vec<String> {
    read_registry()
        .iter()
        .map(|(name, _)| name.clone())
        .collect()
}

/// The source claiming `extension`, if any. Earlier registrations win.
pub fn table_source_for(extension: &str) -> Option<Arc<dyn TableSource>> {
    read_registry()
        .iter()
        .find(|(_, source)| {
            source
                .extensions()
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(extension))
        })
        .map(|(_, source)| Arc::clone(source))
}

/// Extensions claimed by registered sources, deduplicated in registration order.
pub fn registered_extensions() -> Vec<String> {
    let mut extensions = Vec::new();
    for (_, source) in read_registry().iter() {
        for extension in source.extensions() {
            let extension = extension.to_ascii_lowercase();
            if !extensions.contains(&extension) {
                extensions.push(extension);
            }
        }
    }
    extensions
}

/// True when a registered source claims `extension`.
pub fn has_table_source(extension: &str) -> bool {
    table_source_for(extension).is_some()
}

/// Render extracted tables as documents, one per table.
///
/// Metadata mirrors the spreadsheet loader (`row_count`, `column_count`) plus
/// `table_name`/`table_index`/`total_tables`/`row_offset`, so callers can cite
/// the exact table and row range a chunk came from.
pub fn documents_from_tables(
    tables: Vec<TableData>,
    extension: &str,
    source: Option<&str>,
) -> Vec<Document> {
    let total_tables = tables.len();
    let mut documents = Vec::with_capacity(total_tables);

    for (index, table) in tables.into_iter().enumerate() {
        let content = render_markdown_table(&table.columns, &table.rows);
        if content.is_empty() {
            continue;
        }

        let mut metadata = source_metadata(source);
        metadata.insert("format".to_string(), Value::from("table"));
        metadata.insert("table_format".to_string(), Value::from(extension));
        metadata.insert("table_index".to_string(), Value::from(index));
        metadata.insert("total_tables".to_string(), Value::from(total_tables));
        metadata.insert("row_count".to_string(), Value::from(table.rows.len()));
        metadata.insert("column_count".to_string(), Value::from(table.columns.len()));
        if let Some(name) = table.name {
            metadata.insert("table_name".to_string(), Value::from(name));
        }
        if let Some(row_offset) = table.row_offset {
            metadata.insert("row_offset".to_string(), Value::from(row_offset));
        }

        documents.push(Document::new(content, metadata));
    }

    documents
}

/// Render a header and rows as a Markdown table.
///
/// Shared with the spreadsheet loader so extension output is identical: empty
/// header cells become `column_<n>`, cells are escaped (pipes, newlines) and
/// fully blank rows are dropped.
pub(crate) fn render_markdown_table(header: &[String], rows: &[Vec<String>]) -> String {
    if header.is_empty() {
        return String::new();
    }

    let header: Vec<String> = header
        .iter()
        .enumerate()
        .map(|(index, name)| {
            if name.trim().is_empty() {
                format!("column_{}", index + 1)
            } else {
                escape_cell(name)
            }
        })
        .collect();

    let mut out = String::new();
    out.push_str("| ");
    out.push_str(&header.join(" | "));
    out.push_str(" |\n| ");
    out.push_str(&vec!["---"; header.len()].join(" | "));
    out.push_str(" |");

    for row in rows {
        let cells: Vec<String> = row.iter().map(|cell| escape_cell(cell)).collect();
        // Skip fully blank rows so trailing table space does not inflate chunks.
        if cells.iter().all(String::is_empty) {
            continue;
        }
        out.push_str("\n| ");
        out.push_str(&cells.join(" | "));
        out.push_str(" |");
    }

    out
}

/// Cell text as Markdown-safe content.
fn escape_cell(cell: &str) -> String {
    cell.replace('|', "\\|").replace(['\r', '\n'], " ")
}
