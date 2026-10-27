//! Archive expansion.
//!
//! An archive is not a document, so it is expanded: every member is dispatched by
//! its own name through the normal extractors and the resulting documents carry
//! `archive_entry` (their path inside the archive) so a chunk can still be cited
//! back to a member. Nested archives are expanded up to a depth limit.
//!
//! Containers are recognised by their magic bytes, not by their extension, so a
//! mislabelled or extensionless payload still expands. Supported: zip, 7z, tar,
//! tar compressed with gzip/bzip2/xz/zstd, and a single compressed file
//! (`data.csv.gz`, `dump.jsonl.zst`) which becomes one document. RAR is recognised
//! and refused with advice — there is no pure-Rust reader for it, and this crate
//! takes no C dependency.
//!
//! Limits are enforced on declared sizes *before* an entry is read where the
//! container declares them (zip, 7z, tar headers), and while decompressing where it
//! cannot (gzip, bzip2, xz, zstd), so a compression bomb is rejected rather than
//! expanded.

use std::io::{self, Cursor, Read};

use serde_json::Value;

use crate::error::ChunkrError;
use crate::structures::document::Document;

/// Dictionary ceiling for xz streams, in KiB. A stream may declare its own
/// dictionary; without a ceiling a small file could ask for gigabytes.
const XZ_MEMORY_LIMIT_KB: u32 = 256 * 1024;

/// Bytes of a stream decoded to find out whether it holds a tar archive.
const TAR_HEAD_BYTES: usize = 512;

/// What an archive is allowed to contain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveLimits {
    /// How many archives deep to expand (`2` = a zip inside a zip). A compressed
    /// tar (`.tar.gz`, `.tgz`) counts as one archive, not two.
    pub max_depth: usize,
    /// Members examined in one archive.
    pub max_entries: usize,
    /// Total uncompressed payload accepted from one archive.
    pub max_uncompressed_bytes: u64,
    /// Largest uncompressed:compressed ratio accepted per entry, for entries
    /// above `ratio_floor_bytes` (small text files legitimately compress well).
    ///
    /// Only zip states both sizes per entry; for tar and 7z the declared
    /// uncompressed total is checked instead, and stream formats are cut off
    /// while decompressing.
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

/// Archive containers recognised by magic bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Container {
    Zip,
    Tar,
    SevenZ,
    Gzip,
    Bzip2,
    Xz,
    Zstd,
    Rar,
}

/// Expand any supported archive payload into documents, one extraction per member.
///
/// `depth` is the number of archives already expanded above this one; members that
/// are themselves archives recurse until [`ArchiveLimits::max_depth`].
pub fn expand(
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

    match sniff_container(bytes) {
        Some(Container::Zip) => expand_zip(bytes, source, limits, depth),
        Some(Container::Tar) => expand_tar(Cursor::new(bytes), source, limits, depth),
        Some(Container::SevenZ) => expand_sevenz(bytes, source, limits, depth),
        Some(
            container @ (Container::Gzip | Container::Bzip2 | Container::Xz | Container::Zstd),
        ) => expand_stream(bytes, container, source, limits, depth),
        Some(Container::Rar) => Err(archive_error(
            "RAR needs an external decoder and this crate takes no C dependency; \
             extract it first (zip, 7z, tar and compressed tar are read directly)"
                .to_string(),
        )),
        None => Err(archive_error(
            "unrecognised archive format: expected zip, 7z, tar, tar.gz/bz2/xz/zst or a \
             compressed file"
                .to_string(),
        )),
    }
}

/// Expand a zip payload.
fn expand_zip(
    bytes: &[u8],
    source: Option<&str>,
    limits: &ArchiveLimits,
    depth: usize,
) -> Result<Vec<Document>, ChunkrError> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| archive_error(format!("{e}")))?;
    let entries = archive.len();
    if entries > limits.max_entries {
        return Err(archive_error(format!(
            "{entries} entries exceed the {} entry limit",
            limits.max_entries
        )));
    }

    let mut expansion = Expansion::new(source, limits, depth, entries);
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

        expansion.member(&entry, &data)?;
    }

    Ok(expansion.finish())
}

/// Expand a 7z payload.
///
/// 7z states every member's uncompressed size in its header, from before any block
/// is decoded, so the limits are settled up front — a solid archive cannot be
/// partially skipped, which makes reading the header first the only cheap defence.
fn expand_sevenz(
    bytes: &[u8],
    source: Option<&str>,
    limits: &ArchiveLimits,
    depth: usize,
) -> Result<Vec<Document>, ChunkrError> {
    let mut reader =
        sevenz_rust2::ArchiveReader::new(Cursor::new(bytes), sevenz_rust2::Password::empty())
            .map_err(|e| archive_error(format!("{e}")))?;

    let files = &reader.archive().files;
    let entries = files.len();
    if entries > limits.max_entries {
        return Err(archive_error(format!(
            "{entries} entries exceed the {} entry limit",
            limits.max_entries
        )));
    }
    let declared: u64 = files
        .iter()
        .filter(|file| !file.is_directory)
        .map(|file| file.size)
        .fold(0u64, u64::saturating_add);
    if declared > limits.max_uncompressed_bytes {
        return Err(archive_error(format!(
            "uncompressed content exceeds {} MiB",
            limits.max_uncompressed_bytes / (1024 * 1024)
        )));
    }

    let mut expansion = Expansion::new(source, limits, depth, entries);
    let mut failure: Option<ChunkrError> = None;
    let each = |entry: &sevenz_rust2::ArchiveEntry, member: &mut dyn Read| {
        if entry.is_directory || is_archive_noise(&entry.name) {
            return Ok(true);
        }
        let mut data = Vec::with_capacity(entry.size.min(limits.ratio_floor_bytes) as usize);
        if let Err(error) = member.read_to_end(&mut data) {
            failure = Some(archive_error(format!("entry '{}': {error}", entry.name)));
            return Ok(false);
        }
        if let Err(error) = expansion.member(&entry.name, &data) {
            failure = Some(error);
            return Ok(false);
        }
        Ok(true)
    };
    let walked = reader.for_each_entries(each);
    if let Some(error) = failure {
        return Err(error);
    }
    walked.map_err(|e| archive_error(format!("{e}")))?;

    Ok(expansion.finish())
}

/// Expand a tar payload (compressed or not), one extraction per member.
fn expand_tar<R: Read>(
    reader: R,
    source: Option<&str>,
    limits: &ArchiveLimits,
    depth: usize,
) -> Result<Vec<Document>, ChunkrError> {
    let mut archive = tar::Archive::new(reader);
    let mut expansion = Expansion::new(source, limits, depth, 0);
    let mut total_bytes = 0u64;

    let entries = archive
        .entries()
        .map_err(|e| archive_error(format!("{e}")))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| archive_error(format!("{e}")))?;
        expansion.entries += 1;
        if expansion.entries > limits.max_entries {
            return Err(archive_error(format!(
                "entry count exceeds the {} entry limit",
                limits.max_entries
            )));
        }
        // Directories, links, pax headers: nothing to extract, and following a link
        // out of the archive is not something this loader ever does.
        if !entry.header().entry_type().is_file() {
            continue;
        }

        let name = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        if is_archive_noise(&name) {
            continue;
        }
        let size = entry.header().size().unwrap_or(0);
        total_bytes = total_bytes.saturating_add(size);
        if total_bytes > limits.max_uncompressed_bytes {
            return Err(archive_error(format!(
                "uncompressed content exceeds {} MiB",
                limits.max_uncompressed_bytes / (1024 * 1024)
            )));
        }

        let mut data = Vec::with_capacity(size.min(limits.ratio_floor_bytes) as usize);
        entry
            .read_to_end(&mut data)
            .map_err(|e| archive_error(format!("entry '{name}': {e}")))?;
        expansion.member(&name, &data)?;
    }

    Ok(expansion.finish())
}

/// Expand a single-stream compression container.
///
/// Two shapes arrive this way: a compressed archive (`.tar.gz`, `.tgz`, `.tar.zst`)
/// and a compressed *document* (`.csv.gz`, `.jsonl.bz2`, `.xml.xz`). The first is
/// streamed — the tar is read through the decoder, so a 2 GB dump never exists in
/// memory twice — and the second is decoded to bytes and dispatched like any other
/// payload, using the name with the compression suffix removed.
fn expand_stream(
    bytes: &[u8],
    container: Container,
    source: Option<&str>,
    limits: &ArchiveLimits,
    depth: usize,
) -> Result<Vec<Document>, ChunkrError> {
    if looks_like_tar(&read_head(bytes, container, TAR_HEAD_BYTES)?) {
        let reader = Budgeted::new(decode(bytes, container)?, limits.max_uncompressed_bytes);
        return expand_tar(reader, source, limits, depth);
    }

    let data = read_all(bytes, container, limits)?;
    let name = inner_name(source, container);
    let name = name.as_deref();

    // A compressed archive is one more container to expand...
    if sniff_container(&data).is_some_and(|inner| inner != Container::Rar) {
        return expand(&data, source, limits, depth + 1);
    }

    // ...anything else is the document itself, one level below the compressor.
    let entry = name.unwrap_or("payload");
    let mut expansion = Expansion::new(source, limits, depth, 1);
    expansion.member(entry, &data)?;
    Ok(expansion.finish())
}

/// Members of one archive, dispatched and annotated as they are read.
struct Expansion<'a> {
    source: Option<&'a str>,
    limits: &'a ArchiveLimits,
    depth: usize,
    entries: usize,
    skipped: usize,
    documents: Vec<Document>,
}

impl<'a> Expansion<'a> {
    fn new(
        source: Option<&'a str>,
        limits: &'a ArchiveLimits,
        depth: usize,
        entries: usize,
    ) -> Self {
        Self {
            source,
            limits,
            depth,
            entries,
            skipped: 0,
            documents: Vec::new(),
        }
    }

    /// Extract one member, keeping its name so a chunk can be cited back to it.
    fn member(&mut self, name: &str, data: &[u8]) -> Result<(), ChunkrError> {
        let entry_source = match self.source {
            Some(archive_source) => format!("{archive_source}!{name}"),
            None => name.to_string(),
        };
        match crate::loader::auto::extract_payload(
            data,
            &crate::loader::auto::extension_of(name),
            Some(&entry_source),
            self.depth + 1,
            Some(self.limits),
        ) {
            Ok(documents) => {
                for mut document in documents {
                    document
                        .metadata
                        .entry("archive_entry".to_string())
                        .or_insert_with(|| Value::from(name));
                    self.documents.push(document);
                }
                Ok(())
            }
            // A member we cannot read is not a reason to fail the archive: a
            // screenshot inside a zip is expected to be unreadable as text.
            Err(error) if is_unsupported_payload(&error) => {
                self.skipped += 1;
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    /// Attach the counts that are only known once the archive has been walked.
    fn finish(mut self) -> Vec<Document> {
        for document in self.documents.iter_mut() {
            document
                .metadata
                .insert("archive_entries".to_string(), Value::from(self.entries));
            document
                .metadata
                .insert("archive_skipped".to_string(), Value::from(self.skipped));
        }
        self.documents
    }
}

impl Container {
    /// Extension that names this container, for payloads that arrived without one.
    pub(crate) fn extension(self) -> &'static str {
        match self {
            Container::Zip => "zip",
            Container::Tar => "tar",
            Container::SevenZ => "7z",
            Container::Gzip => "gz",
            Container::Bzip2 => "bz2",
            Container::Xz => "xz",
            Container::Zstd => "zst",
            Container::Rar => "rar",
        }
    }
}

/// The container a payload holds, from its magic bytes.
pub(crate) fn sniff_container(bytes: &[u8]) -> Option<Container> {
    let prefix = |magic: &[u8]| bytes.starts_with(magic);
    if prefix(b"PK\x03\x04") || prefix(b"PK\x05\x06") || prefix(b"PK\x07\x08") {
        return Some(Container::Zip);
    }
    if prefix(b"7z\xBC\xAF\x27\x1C") {
        return Some(Container::SevenZ);
    }
    if prefix(b"Rar!\x1A\x07") {
        return Some(Container::Rar);
    }
    if prefix(b"\x1f\x8b") {
        return Some(Container::Gzip);
    }
    if prefix(b"BZh") {
        return Some(Container::Bzip2);
    }
    if prefix(b"\xFD7zXZ\x00") {
        return Some(Container::Xz);
    }
    if prefix(b"\x28\xB5\x2F\xFD") {
        return Some(Container::Zstd);
    }
    if looks_like_tar(bytes) {
        return Some(Container::Tar);
    }
    None
}

/// True for a POSIX tar header: `ustar` sits at byte 257 in ustar, pax and GNU.
fn looks_like_tar(bytes: &[u8]) -> bool {
    bytes.len() >= 262 && &bytes[257..262] == b"ustar"
}

/// A decoder for one stream container.
fn decode<'a>(bytes: &'a [u8], container: Container) -> Result<Box<dyn Read + 'a>, ChunkrError> {
    let cursor = Cursor::new(bytes);
    Ok(match container {
        Container::Gzip => Box::new(flate2::read::GzDecoder::new(cursor)),
        Container::Bzip2 => Box::new(bzip2_rs::DecoderReader::new(cursor)),
        Container::Xz => Box::new(lzma_rust2::XzReader::new_mem_limit(
            cursor,
            false,
            XZ_MEMORY_LIMIT_KB,
        )),
        Container::Zstd => Box::new(
            ruzstd::decoding::StreamingDecoder::new(cursor)
                .map_err(|e| archive_error(format!("{e}")))?,
        ),
        _ => return Err(archive_error("not a compression stream".to_string())),
    })
}

/// Decode the first `want` bytes of a stream, enough to tell tar from a document.
fn read_head(bytes: &[u8], container: Container, want: usize) -> Result<Vec<u8>, ChunkrError> {
    let mut reader = decode(bytes, container)?;
    let mut head = vec![0u8; want];
    let mut filled = 0;
    while filled < want {
        match reader.read(&mut head[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) => return Err(archive_error(format!("{error}"))),
        }
    }
    head.truncate(filled);
    Ok(head)
}

/// Decode a whole stream, refusing to go past the archive budget.
fn read_all(
    bytes: &[u8],
    container: Container,
    limits: &ArchiveLimits,
) -> Result<Vec<u8>, ChunkrError> {
    let mut reader = Budgeted::new(decode(bytes, container)?, limits.max_uncompressed_bytes);
    let mut data = Vec::new();
    reader
        .read_to_end(&mut data)
        .map_err(|error| archive_error(format!("{error}")))?;
    Ok(data)
}

/// A reader that fails once a stream has produced more than an archive may hold.
///
/// Stream formats do not declare their uncompressed size, so the only place to
/// stop a bomb is while expanding it.
struct Budgeted<R> {
    inner: R,
    allowed: u64,
    produced: u64,
}

impl<R: Read> Budgeted<R> {
    fn new(inner: R, allowed: u64) -> Self {
        Self {
            inner,
            allowed,
            produced: 0,
        }
    }
}

impl<R: Read> Read for Budgeted<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let remaining = self.allowed.saturating_sub(self.produced);
        if remaining == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "uncompressed content exceeds {} MiB",
                    self.allowed / (1024 * 1024)
                ),
            ));
        }
        let want = buffer.len().min(remaining as usize);
        let read = self.inner.read(&mut buffer[..want])?;
        self.produced += read as u64;
        Ok(read)
    }
}

/// The name a compressed document has inside its compressor: `data.csv.gz` →
/// `data.csv`, `dump.jsonl.zst` → `dump.jsonl`. Extensionless names yield nothing,
/// so the payload is sniffed on its own.
fn inner_name(source: Option<&str>, container: Container) -> Option<String> {
    let source = source?;
    let file = source.rsplit(['/', '\\', '!']).next().unwrap_or(source);
    let suffixes: &[&str] = match container {
        Container::Gzip => &[".tgz", ".gz"],
        Container::Bzip2 => &[".tbz2", ".tbz", ".bz2"],
        Container::Xz => &[".txz", ".xz"],
        Container::Zstd => &[".tzst", ".zst"],
        _ => return None,
    };
    for suffix in suffixes {
        if let Some(stem) = file.strip_suffix(suffix) {
            // A `.tgz` is a tar with no inner name of its own.
            return if stem.is_empty() {
                None
            } else {
                Some(stem.to_string())
            };
        }
    }
    // `.tar.gz` keeps its tar name, which the tar sniffing then ignores.
    Some(file.to_string())
}

/// MacOS resource forks and Finder metadata, not documents.
fn is_archive_noise(entry: &str) -> bool {
    entry.starts_with("__MACOSX/") || entry.ends_with("/.DS_Store") || entry == ".DS_Store"
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
