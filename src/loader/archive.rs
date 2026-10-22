//! Zip archive expansion.
//!
//! An archive is not a document, so it is expanded: every entry is dispatched by
//! its own extension through the normal extractors and the resulting documents
//! carry `archive_entry` (their path inside the archive) so a chunk can still be
//! cited back to a member. Nested archives are expanded up to a depth limit.
//!
//! Limits are enforced *before* an entry is read, so a zip bomb is rejected on
//! its declared sizes rather than after the memory is spent.

use std::io::{Cursor, Read};

use serde_json::Value;
use zip::ZipArchive;

use crate::error::ChunkrError;
use crate::structures::document::Document;

/// What an archive is allowed to contain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveLimits {
    /// How many archives deep to expand (`2` = a zip inside a zip).
    pub max_depth: usize,
    /// Members examined in one archive.
    pub max_entries: usize,
    /// Total uncompressed payload accepted from one archive.
    pub max_uncompressed_bytes: u64,
    /// Largest uncompressed:compressed ratio accepted per entry, for entries
    /// above `ratio_floor_bytes` (small text files legitimately compress well).
    pub max_compression_ratio: u64,
    /// Size below which the compression ratio is not checked.
    pub ratio_floor_bytes: u64,
}

impl ArchiveLimits {
    /// Defaults with any `CHUNKR_ARCHIVE_*` overrides applied.
    ///
    /// Deeply nested bundles are a legitimate export shape (a zip of zips of
    /// CSVs), but the safe default is shallow; the env override lets a pipeline
    /// raise it without a rebuild and without turning zip-bomb protection off.
    pub fn from_env() -> Self {
        let mut limits = Self::default();
        if let Some(depth) = env_number("CHUNKR_ARCHIVE_DEPTH") {
            limits.max_depth = depth as usize;
        }
        if let Some(entries) = env_number("CHUNKR_ARCHIVE_ENTRIES") {
            limits.max_entries = entries as usize;
        }
        if let Some(bytes) = env_number("CHUNKR_ARCHIVE_MB") {
            limits.max_uncompressed_bytes = bytes.saturating_mul(1024 * 1024);
        }
        limits
    }
}

/// Parse a non-negative integer env var, ignoring junk rather than failing a load.
fn env_number(name: &str) -> Option<u64> {
    std::env::var(name).ok()?.trim().parse::<u64>().ok()
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_depth: 2,
            max_entries: 1_000,
            max_uncompressed_bytes: 512 * 1024 * 1024,
            max_compression_ratio: 100,
            ratio_floor_bytes: 1024 * 1024,
        }
    }
}

/// Expand a zip payload into documents, one extraction per member.
///
/// `depth` is the number of archives already expanded above this one; entries
/// that are themselves archives recurse until [`ArchiveLimits::max_depth`].
pub fn expand_zip(
    bytes: &[u8],
    source: Option<&str>,
    limits: &ArchiveLimits,
    depth: usize,
) -> Result<Vec<Document>, ChunkrError> {
    if depth >= limits.max_depth {
        return Err(archive_error(format!(
            "archive nested more than {} deep",
            limits.max_depth
        )));
    }

    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|e| archive_error(format!("{e}")))?;
    let entries = archive.len();
    if entries > limits.max_entries {
        return Err(archive_error(format!(
            "{entries} entries exceed the {} entry limit",
            limits.max_entries
        )));
    }

    let mut documents = Vec::new();
    let mut skipped = 0usize;
    let mut total_bytes = 0u64;

    for index in 0..entries {
        let mut member = archive
            .by_index(index)
            .map_err(|e| archive_error(format!("entry {index}: {e}")))?;
        let entry = member
            .name()
            .map_err(|e| archive_error(format!("entry {index} name: {e}")))?
            .to_string();
        if member.is_dir() || is_archive_noise(&entry) {
            continue;
        }

        let uncompressed = member.size();
        let compressed = member.compressed_size();
        total_bytes = total_bytes.saturating_add(uncompressed);
        if total_bytes > limits.max_uncompressed_bytes {
            return Err(archive_error(format!(
                "uncompressed content exceeds {} MiB",
                limits.max_uncompressed_bytes / (1024 * 1024)
            )));
        }
        if uncompressed > limits.ratio_floor_bytes
            && compressed > 0
            && uncompressed / compressed > limits.max_compression_ratio
        {
            return Err(archive_error(format!(
                "entry '{entry}' expands {}x, over the {}x limit",
                uncompressed / compressed,
                limits.max_compression_ratio
            )));
        }

        let mut data = Vec::with_capacity(uncompressed.min(limits.ratio_floor_bytes) as usize);
        member
            .read_to_end(&mut data)
            .map_err(|e| archive_error(format!("entry '{entry}': {e}")))?;
        drop(member);

        let entry_source = match source {
            Some(archive_source) => format!("{archive_source}!{entry}"),
            None => entry.clone(),
        };
        let extension = crate::loader::auto::extension_of(&entry);

        match crate::loader::auto::extract_payload(
            &data,
            &extension,
            Some(&entry_source),
            depth + 1,
        ) {
            Ok(mut extracted) => {
                for document in extracted.iter_mut() {
                    annotate(document, &entry, entries, skipped);
                }
                documents.extend(extracted);
            }
            // A member we cannot read is not a reason to fail the archive: a
            // screenshot inside a zip is expected to be unreadable as text.
            Err(error) if is_unsupported_payload(&error) => skipped += 1,
            Err(error) => return Err(error),
        }
    }

    // Skipped counts are only known once every entry has been attempted.
    if skipped > 0 {
        for document in documents.iter_mut() {
            document
                .metadata
                .insert("archive_skipped".to_string(), Value::from(skipped));
        }
    }

    Ok(documents)
}

/// Record where a document came from inside its archive.
///
/// The entry is the innermost member found, so it survives nested expansion;
/// the counts describe the archive as it was handed in, which is what a caller
/// wants to know about the payload they passed.
fn annotate(document: &mut Document, entry: &str, entries: usize, skipped: usize) {
    document
        .metadata
        .entry("archive_entry".to_string())
        .or_insert_with(|| Value::from(entry));
    document
        .metadata
        .insert("archive_entries".to_string(), Value::from(entries));
    document
        .metadata
        .insert("archive_skipped".to_string(), Value::from(skipped));
}

/// MacOS resource forks and Finder metadata, not documents.
fn is_archive_noise(entry: &str) -> bool {
    entry.starts_with("__MACOSX/") || entry.ends_with("/.DS_Store")
}

/// True when the error says the payload is not a format this crate reads.
fn is_unsupported_payload(error: &ChunkrError) -> bool {
    error
        .to_string()
        .contains("binary payload is not a supported format")
}

fn archive_error(message: String) -> ChunkrError {
    ChunkrError::ParseError(format!("archive: {message}"))
}
