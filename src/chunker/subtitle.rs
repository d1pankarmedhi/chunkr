//! Time-indexed chunking for subtitle and caption tracks (SRT, WebVTT).
//!
//! Cues are the atomic unit: a window never splits one, and every chunk carries
//! its source span as `start_time_s` / `end_time_s`, so retrieval can cite the
//! exact moment inside the media file. The same span keys are what audio
//! transcripts use, so both modalities stay filterable in one vector store.

use std::collections::HashMap;

use serde_json::Value;

use crate::chunker::base::Chunker;
use crate::error::ChunkrError;
use crate::structures::document::Document;

/// Target window length in seconds; a single longer cue is never split.
pub const DEFAULT_MAX_SECONDS: f64 = 30.0;
/// Target window length in characters; a single longer cue is never split.
pub const DEFAULT_MAX_CHARACTERS: usize = 1000;

/// One subtitle cue with its span in the source track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cue {
    /// Milliseconds from the start of the track.
    pub start_ms: u64,
    /// Milliseconds from the start of the track.
    pub end_ms: u64,
    /// Cue text with markup stripped and entities unescaped.
    pub text: String,
    /// WebVTT voice tag (`<v Roger>`), when the track names the speaker.
    pub speaker: Option<String>,
}

/// Chunks subtitle tracks into time windows that never split a cue.
#[derive(Debug, Clone)]
pub struct SubtitleChunker {
    pub max_seconds: f64,
    pub max_characters: usize,
}

impl SubtitleChunker {
    pub fn new() -> Self {
        Self {
            max_seconds: DEFAULT_MAX_SECONDS,
            max_characters: DEFAULT_MAX_CHARACTERS,
        }
    }

    /// Set the target window length in seconds.
    pub fn with_max_seconds(mut self, max_seconds: f64) -> Self {
        self.max_seconds = max_seconds;
        self
    }

    /// Set the target window length in characters.
    pub fn with_max_characters(mut self, max_characters: usize) -> Self {
        self.max_characters = max_characters;
        self
    }

    /// Group cues into windows and build one chunk per window.
    pub fn windows(&self, cues: &[Cue]) -> Vec<Document> {
        let mut chunks = Vec::new();
        let mut start = 0usize;

        while start < cues.len() {
            // The first cue always belongs to the window, even when it alone
            // exceeds a budget: cues stay atomic.
            let mut end = start + 1;
            let mut characters = cues[start].text.len();
            while end < cues.len() {
                let cue = &cues[end];
                let projected = characters + 1 + cue.text.len();
                let seconds = (cue.end_ms.saturating_sub(cues[start].start_ms)) as f64 / 1000.0;
                if seconds > self.max_seconds || projected > self.max_characters {
                    break;
                }
                characters = projected;
                end += 1;
            }

            chunks.push(window_document(&cues[start..end]));
            start = end;
        }

        chunks
    }
}

impl Default for SubtitleChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunker for SubtitleChunker {
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError> {
        if text.trim().is_empty() {
            return Err(ChunkrError::EmptyInput);
        }
        Ok(self.windows(&parse_cues(text)?))
    }

    /// Merge cue documents from a subtitle loader straight into windows.
    ///
    /// Non-cue documents fall back to parsing each document's text as a track.
    fn chunk_documents(&self, docs: &[Document]) -> Result<Vec<Document>, ChunkrError> {
        if !docs.iter().all(is_cue_document) {
            return chunk_text_documents(self, docs);
        }

        let cues: Vec<Cue> = docs.iter().map(cue_from_document).collect();
        let mut chunks = self.windows(&cues);

        // Identity metadata (path, format, track size) belongs on every window;
        // cue_index deliberately does not — each window has its own cue span.
        if let Some(first) = docs.first() {
            for (index, chunk) in chunks.iter_mut().enumerate() {
                for key in INHERITED_KEYS {
                    if let Some(value) = first.metadata.get(*key) {
                        chunk.metadata.insert((*key).to_string(), value.clone());
                    }
                }
                chunk
                    .metadata
                    .insert("chunk_index".to_string(), Value::from(index));
            }
        }

        Ok(chunks)
    }
}

/// Identity keys a merged window inherits from the cue documents it came from.
const INHERITED_KEYS: &[&str] = &[
    "source",
    "file_name",
    "file_extension",
    "format",
    "subtitle_format",
    "total_cues",
];

/// True when a document is one cue carrying its own span.
fn is_cue_document(document: &Document) -> bool {
    document.metadata.get("format").and_then(Value::as_str) == Some("subtitle")
        && document.metadata.get("start_time_s").and_then(Value::as_f64).is_some()
        && document.metadata.get("end_time_s").and_then(Value::as_f64).is_some()
}

fn cue_from_document(document: &Document) -> Cue {
    Cue {
        start_ms: millis(document, "start_time_s"),
        end_ms: millis(document, "end_time_s"),
        text: document.content.trim().to_string(),
        speaker: document
            .metadata
            .get("speaker")
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

fn millis(document: &Document, key: &str) -> u64 {
    let seconds = document
        .metadata
        .get(key)
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    (seconds.max(0.0) * 1000.0).round() as u64
}

/// Default per-document behaviour for payloads that are not cue documents.
fn chunk_text_documents<C: Chunker>(chunker: &C, docs: &[Document]) -> Result<Vec<Document>, ChunkrError> {
    let mut all_chunks = Vec::new();
    for (doc_index, document) in docs.iter().enumerate() {
        if document.content.trim().is_empty() {
            continue;
        }
        for (chunk_index, mut chunk) in chunker.chunk(&document.content)?.into_iter().enumerate() {
            for (key, value) in &document.metadata {
                chunk.metadata.entry(key.clone()).or_insert_with(|| value.clone());
            }
            chunk
                .metadata
                .insert("doc_index".to_string(), Value::from(doc_index));
            chunk
                .metadata
                .insert("chunk_index".to_string(), Value::from(chunk_index));
            all_chunks.push(chunk);
        }
    }
    Ok(all_chunks)
}

fn window_document(window: &[Cue]) -> Document {
    let start_ms = window.first().map(|cue| cue.start_ms).unwrap_or(0);
    let end_ms = window
        .iter()
        .map(|cue| cue.end_ms)
        .max()
        .unwrap_or(start_ms);

    let mut metadata = HashMap::new();
    metadata.insert("start_time_s".to_string(), Value::from(seconds(start_ms)));
    metadata.insert("end_time_s".to_string(), Value::from(seconds(end_ms)));
    metadata.insert(
        "duration_s".to_string(),
        Value::from(seconds(end_ms.saturating_sub(start_ms))),
    );
    metadata.insert("cue_count".to_string(), Value::from(window.len()));
    if let Some(speaker) = uniform_speaker(window) {
        metadata.insert("speaker".to_string(), Value::from(speaker));
    }

    let content = window
        .iter()
        .map(|cue| cue.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    Document::new(content, metadata)
}

/// The speaker of a window, when every voiced cue agrees on one.
fn uniform_speaker(window: &[Cue]) -> Option<String> {
    let mut speakers: Vec<&str> = Vec::new();
    for speaker in window.iter().filter_map(|cue| cue.speaker.as_deref()) {
        if !speakers.contains(&speaker) {
            speakers.push(speaker);
        }
    }
    match speakers.as_slice() {
        [speaker] => Some((*speaker).to_string()),
        _ => None,
    }
}

fn seconds(milliseconds: u64) -> f64 {
    milliseconds as f64 / 1000.0
}

/// Parse an SRT or WebVTT track into cues.
///
/// One tolerant block parser handles both: the `WEBVTT` header, `NOTE`/`STYLE`/
/// `REGION` blocks and SRT cue indices all parse to blocks without a timing line
/// and are dropped.
pub fn parse_cues(text: &str) -> Result<Vec<Cue>, ChunkrError> {
    let text = text.trim_start_matches('\u{feff}');
    let cues = parse_blocks(text);

    if cues.is_empty() {
        return Err(ChunkrError::ParseError(
            "subtitle: no cues found (expected SRT or WebVTT)".to_string(),
        ));
    }
    Ok(cues)
}

/// One cue per blank-line-separated block; blocks carrying no timing are dropped.
fn parse_blocks(text: &str) -> Vec<Cue> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    normalized.split("\n\n").filter_map(parse_block).collect()
}

fn parse_block(block: &str) -> Option<Cue> {
    let mut span: Option<(u64, u64)> = None;
    let mut text = String::new();
    let mut speaker = None;

    for line in block.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match span {
            // Before the timing line only the cue id may appear; metadata blocks
            // (WEBVTT, NOTE, STYLE, REGION) carry no timing and end up dropped.
            None => {
                if let Some(bounds) = parse_timing_line(line) {
                    span = Some(bounds);
                }
            }
            Some(_) => {
                let (stripped, voice) = strip_markup(line);
                if speaker.is_none() {
                    speaker = voice;
                }
                if !stripped.is_empty() {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(&stripped);
                }
            }
        }
    }

    let (start_ms, end_ms) = span?;
    if text.is_empty() {
        return None;
    }
    Some(Cue {
        start_ms,
        end_ms,
        text,
        speaker,
    })
}

/// `00:00:01,000 --> 00:00:04,500 align:start position:0%`
fn parse_timing_line(line: &str) -> Option<(u64, u64)> {
    let (start, rest) = line.split_once("-->")?;
    let start_ms = parse_timestamp(start.trim())?;
    let end_ms = parse_timestamp(rest.split_whitespace().next()?)?;
    Some((start_ms, end_ms))
}

/// `HH:MM:SS,mmm`, `HH:MM:SS.mmm` or VTT's `MM:SS.mmm`.
fn parse_timestamp(value: &str) -> Option<u64> {
    let normalized = value.replace(',', ".");
    let mut parts = normalized.split(':');
    let first = parts.next()?;
    let (hours, minutes, seconds) = match (parts.next(), parts.next(), parts.next()) {
        (Some(minutes), Some(seconds), None) => (first.parse::<u64>().ok()?, minutes.parse::<u64>().ok()?, seconds),
        (Some(seconds), None, None) => (0, first.parse::<u64>().ok()?, seconds),
        _ => return None,
    };

    let (whole, fraction) = match seconds.split_once('.') {
        Some((whole, fraction)) => (whole.parse::<u64>().ok()?, fraction),
        None => (seconds.parse::<u64>().ok()?, ""),
    };
    Some(((hours * 60 + minutes) * 60 + whole) * 1000 + to_millis(fraction))
}

/// Right-pad the fraction so `.5` reads as 500 ms and `.25` as 250 ms.
fn to_millis(fraction: &str) -> u64 {
    let digits: String = fraction.chars().take(3).collect();
    format!("{digits:0<3}").parse().unwrap_or(0)
}

/// Strip cue markup (`<i>`, `<c.class>`, `<00:00:01.000>`, `{\an8}`), returning
/// the spoken text and the WebVTT voice tag when the cue declares one.
fn strip_markup(line: &str) -> (String, Option<String>) {
    let mut out = String::with_capacity(line.len());
    let mut speaker = None;
    let mut rest = line;

    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else {
            out.push_str(&rest[open..]);
            rest = "";
            break;
        };
        let tag = rest[open + 1..open + close].trim();
        if let Some(name) = tag.strip_prefix('v').map(str::trim) {
            if !name.is_empty() && speaker.is_none() {
                speaker = Some(name.to_string());
            }
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);

    (unescape(&strip_braces(&out)), speaker)
}

/// SRT position/style groups such as `{\an8}` carry no spoken text.
fn strip_braces(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        match rest[open..].find('}') {
            Some(close) => rest = &rest[open + close + 1..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The entity set subtitle files actually use.
fn unescape(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('&') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find(';') else {
            out.push_str(&rest[open..]);
            rest = "";
            break;
        };
        let entity = &rest[open + 1..open + close];
        match entity {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" | "#39" => out.push('\''),
            "nbsp" => out.push(' '),
            _ => out.push_str(&rest[open..open + close + 1]),
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}
