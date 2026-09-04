//! # Post-Chunking Transformation Pipeline
//!
//! This module provides a composable pipeline for post-processing, optimizing, and
//! enriching chunks produced by any chunker.
//!
//! ## Pipeline Stages
//!
//! - [`ChunkFilter`]: Filters out low-quality chunks by character length, word count,
//!   or alphanumeric ratio (removes noise, boilerplate, formatting junk).
//! - [`ChunkDeduplicator`]: Deduplicates identical chunks based on content hashes.
//! - [`ChunkPacker`](crate::chunker::packer::ChunkPacker): Bin-packs small chunks
//!   into target budget windows to avoid fragmentation.
//! - [`MetadataEnricher`]: Calculates deterministic SHA-256 chunk IDs, character/word
//!   metrics, and timestamps.
//!
//! ## Example
//!
//! ```rust
//! use chunkr::prelude::*;
//!
//! let pipeline = ChunkPipeline::new()
//!     .filter_min_characters(30)
//!     .filter_min_alpha_ratio(0.5)
//!     .deduplicate_exact(true)
//!     .pack(1000)
//!     .enrich_metadata();
//! ```

pub mod chain;
pub mod dedup;
pub mod enrich;
pub mod filter;

pub use chain::ChunkPipeline;
pub use dedup::ChunkDeduplicator;
pub use enrich::MetadataEnricher;
pub use filter::ChunkFilter;
