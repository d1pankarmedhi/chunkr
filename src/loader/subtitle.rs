//! Subtitle extraction for SRT and WebVTT tracks.
//!
//! One [`Document`] per cue, with the cue's span as `start_time_s` / `end_time_s`
//! metadata, so cue granularity is available directly and
//! [`SubtitleChunker`](crate::chunker::subtitle::SubtitleChunker) — which
//! `DirectoryLoader` routes these extensions to — can merge cues into time
//! windows without re-parsing anything.

use std::path::Path;

use serde_json::Value;

use crate::chunker::subtitle::parse_cues;
use crate::error::ChunkrError;
use crate::loader::base::source_metadata;
use crate::loader::text::decode_text;
use crate::structures::document::Document;

/// Extracts cue text from subtitle tracks.
#[derive(Debug, Clone, Default)]
pub struct SubtitleLoader {}

impl SubtitleLoader {
    pub fn new() -> Self {
        Self {}
    }

    /// Load an SRT or WebVTT file into one document per cue.
    pub fn load_from_file<P: AsRef<Path>>(&self, path: P) -> Result<Vec<Document>, ChunkrError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)?;
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("srt");
        self.load_documents(&bytes, extension, Some(&path.to_string_lossy()))
    }

    /// Load a subtitle payload held in memory, one document per cue.
    pub fn load_documents(
        &self,
        bytes: &[u8],
        extension: &str,
        source: Option<&str>,
    ) -> Result<Vec<Document>, ChunkrError> {
        // Legacy single-byte encodings are common in subtitle tooling, and
        // `decode_text` also honours byte-order marks.
        let cues = parse_cues(&decode_text(bytes))?;
        let subtitle_format = subtitle_format(extension);
        let total_cues = cues.len();

        let documents = cues
            .iter()
            .enumerate()
            .map(|(index, cue)| {
                let mut metadata = source_metadata(source);
                metadata.insert("format".to_string(), Value::from("subtitle"));
                metadata.insert("subtitle_format".to_string(), Value::from(subtitle_format));
                metadata.insert("cue_index".to_string(), Value::from(index));
                metadata.insert("total_cues".to_string(), Value::from(total_cues));
                metadata.insert(
                    "start_time_s".to_string(),
                    Value::from(seconds(cue.start_ms)),
                );
                metadata.insert("end_time_s".to_string(), Value::from(seconds(cue.end_ms)));
                metadata.insert(
                    "duration_s".to_string(),
                    Value::from(seconds(cue.end_ms.saturating_sub(cue.start_ms))),
                );
                if let Some(speaker) = cue.speaker.clone() {
                    metadata.insert("speaker".to_string(), Value::from(speaker));
                }
                Document::new(cue.text.clone(), metadata)
            })
            .collect();

        Ok(documents)
    }
}

fn subtitle_format(extension: &str) -> &'static str {
    if extension.eq_ignore_ascii_case("srt") {
        "srt"
    } else {
        "vtt"
    }
}

fn seconds(milliseconds: u64) -> f64 {
    milliseconds as f64 / 1000.0
}
