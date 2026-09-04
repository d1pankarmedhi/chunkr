//! # Document Loaders
//!
//! This module provides loaders for ingesting documents from the local filesystem,
//! raw directories, and PDF files.
//!
//! - [`BaseLoader`]: Common trait for document loaders.
//! - [`DirectoryLoader`]: Recursively scans directories, filters by file extension,
//!   and automatically routes files to the optimal chunking strategy.
//! - [`PDFLoader`]: Extracts text page-by-page from PDF files into [`Document`](crate::structures::document::Document) structs.

pub mod base;
#[cfg(not(target_arch = "wasm32"))]
pub mod directory;
pub mod pdf;

pub use base::BaseLoader;
#[cfg(not(target_arch = "wasm32"))]
pub use directory::DirectoryLoader;
pub use pdf::PDFLoader;
