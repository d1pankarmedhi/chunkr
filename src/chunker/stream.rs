use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use serde_json::Value;

use crate::error::ChunkrError;
use crate::structures::document::Document;

/// Streaming chunker that processes arbitrary `BufRead` streams with constant memory.
#[derive(Debug, Clone)]
pub struct StreamChunker {
    pub chunk_size: usize,
    pub overlap: usize,
}

impl StreamChunker {
    /// Create a new StreamChunker with specified chunk size and overlap
    pub fn new(chunk_size: usize, overlap: usize) -> Result<Self, ChunkrError> {
        if chunk_size == 0 {
            return Err(ChunkrError::InvalidChunkSize(0));
        }
        if overlap >= chunk_size {
            return Err(ChunkrError::InvalidOverlap {
                chunk_size,
                overlap,
            });
        }
        Ok(Self {
            chunk_size,
            overlap,
        })
    }

    /// Open and stream-chunk a file from disk with constant memory footprint
    pub fn chunk_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<ChunkReaderIterator<BufReader<File>>, ChunkrError> {
        let file = File::open(path).map_err(|e| ChunkrError::IoError(e.to_string()))?;
        let reader = BufReader::new(file);
        Ok(self.chunk_reader(reader))
    }

    /// Stream-chunk any `BufRead` instance (network socket, gzip stream, stdin, file)
    pub fn chunk_reader<R: BufRead>(&self, reader: R) -> ChunkReaderIterator<R> {
        ChunkReaderIterator {
            reader,
            chunk_size: self.chunk_size,
            overlap: self.overlap,
            buffer: String::with_capacity(self.chunk_size * 2),
            chunk_index: 0,
            eof: false,
        }
    }
}

impl Default for StreamChunker {
    fn default() -> Self {
        Self::new(1000, 150).expect("Valid default parameters")
    }
}

/// Iterator that yields Document chunks lazily from an underlying `BufRead` source.
pub struct ChunkReaderIterator<R: BufRead> {
    reader: R,
    chunk_size: usize,
    overlap: usize,
    buffer: String,
    chunk_index: usize,
    eof: bool,
}

impl<R: BufRead> ChunkReaderIterator<R> {
    fn find_cut_point(buffer: &str, target: usize, overlap: usize) -> usize {
        let max_pos = target.min(buffer.len());
        let min_pos = target.saturating_sub(overlap).max(max_pos / 2);
        let search_slice = &buffer[..max_pos];

        if let Some(pos) = search_slice.rfind("\n\n") {
            let cut = pos + 2;
            if cut >= min_pos {
                return cut;
            }
        }
        if let Some(pos) = search_slice.rfind('\n') {
            let cut = pos + 1;
            if cut >= min_pos {
                return cut;
            }
        }
        if let Some(pos) = search_slice.rfind(". ") {
            let cut = pos + 2;
            if cut >= min_pos {
                return cut;
            }
        }
        if let Some(pos) = search_slice.rfind(' ') {
            let cut = pos + 1;
            if cut >= min_pos {
                return cut;
            }
        }

        // Fallback: snap to char boundary near max_pos
        let mut cut = max_pos;
        while cut > 0 && !buffer.is_char_boundary(cut) {
            cut -= 1;
        }
        if cut == 0 && !buffer.is_empty() {
            cut = buffer.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        }
        cut
    }

    fn make_chunk(&mut self, content: String) -> Document {
        let mut metadata = HashMap::with_capacity(4);
        metadata.insert("chunk_index".to_string(), Value::from(self.chunk_index));
        metadata.insert("length".to_string(), Value::from(content.len()));
        metadata.insert("strategy".to_string(), Value::from("stream"));

        self.chunk_index += 1;
        Document { content, metadata }
    }
}

impl<R: BufRead> Iterator for ChunkReaderIterator<R> {
    type Item = Result<Document, ChunkrError>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut line = String::new();

        while !self.eof {
            // Check if buffer has accumulated enough text for a chunk
            if self.buffer.len() >= self.chunk_size {
                let cut = Self::find_cut_point(&self.buffer, self.chunk_size, self.overlap);
                let chunk_text = self.buffer[..cut].trim().to_string();

                let advance = if cut > self.overlap {
                    cut - self.overlap
                } else {
                    cut
                }.max(1);

                let mut safe_advance = advance.min(self.buffer.len());
                while safe_advance < self.buffer.len() && !self.buffer.is_char_boundary(safe_advance) {
                    safe_advance += 1;
                }
                if !self.buffer.is_char_boundary(safe_advance) {
                    safe_advance = self.buffer.len();
                }
                self.buffer.drain(..safe_advance);

                if !chunk_text.is_empty() {
                    return Some(Ok(self.make_chunk(chunk_text)));
                }
            }

            line.clear();
            match self.reader.read_line(&mut line) {
                Ok(0) => {
                    self.eof = true;
                    break;
                }
                Ok(_) => {
                    self.buffer.push_str(&line);
                }
                Err(e) => return Some(Err(ChunkrError::IoError(e.to_string()))),
            }
        }

        // EOF reached: yield any residual text
        while !self.buffer.is_empty() {
            let cut = Self::find_cut_point(&self.buffer, self.chunk_size, self.overlap);
            let chunk_text = self.buffer[..cut].trim().to_string();

            let advance = if cut > self.overlap && self.buffer.len() > self.chunk_size {
                cut - self.overlap
            } else {
                cut
            }.max(1);

            let mut safe_advance = advance.min(self.buffer.len());
            while safe_advance < self.buffer.len() && !self.buffer.is_char_boundary(safe_advance) {
                safe_advance += 1;
            }
            if !self.buffer.is_char_boundary(safe_advance) {
                safe_advance = self.buffer.len();
            }
            self.buffer.drain(..safe_advance);

            if !chunk_text.is_empty() {
                return Some(Ok(self.make_chunk(chunk_text)));
            }
        }

        None
    }
}
