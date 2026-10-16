//! # Document Loaders
//!
//! This module provides loaders for ingesting documents from the local filesystem,
//! raw directories, and individual file formats.
//!
//! - [`BaseLoader`]: Common trait for document loaders.
//! - [`load_documents`]: Extension-dispatched extraction for every supported format.
//! - [`DirectoryLoader`]: Recursively scans directories, filters by file extension,
//!   and automatically routes files to the optimal chunking strategy.
//! - [`PDFLoader`]: Extracts text page-by-page from PDF files into [`Document`](crate::structures::document::Document) structs.
//! - [`SpreadsheetLoader`], [`OfficeLoader`], [`EpubLoader`], [`EmailLoader`]: one
//!   loader per format family, callable directly when the extension is known.

pub mod base;
pub mod pdf;
pub use base::BaseLoader;
pub use pdf::PDFLoader;

#[cfg(not(target_arch = "wasm32"))]
pub mod auto;
#[cfg(not(target_arch = "wasm32"))]
pub mod directory;
#[cfg(not(target_arch = "wasm32"))]
pub mod email;
#[cfg(not(target_arch = "wasm32"))]
pub mod epub;
#[cfg(not(target_arch = "wasm32"))]
pub mod office;
#[cfg(not(target_arch = "wasm32"))]
pub mod spreadsheet;
#[cfg(not(target_arch = "wasm32"))]
pub mod text;
#[cfg(not(target_arch = "wasm32"))]
pub mod web;

#[cfg(not(target_arch = "wasm32"))]
pub use auto::{
    is_structured, load_documents, load_documents_from_bytes, supported_extensions,
    STRUCTURED_EXTENSIONS,
};
#[cfg(not(target_arch = "wasm32"))]
pub use directory::DirectoryLoader;
#[cfg(not(target_arch = "wasm32"))]
pub use email::EmailLoader;
#[cfg(not(target_arch = "wasm32"))]
pub use epub::EpubLoader;
#[cfg(not(target_arch = "wasm32"))]
pub use office::OfficeLoader;
#[cfg(not(target_arch = "wasm32"))]
pub use spreadsheet::SpreadsheetLoader;
