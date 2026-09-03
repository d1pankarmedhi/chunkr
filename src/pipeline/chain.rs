use crate::chunker::packer::ChunkPacker;
use crate::pipeline::dedup::ChunkDeduplicator;
use crate::pipeline::enrich::MetadataEnricher;
use crate::pipeline::filter::ChunkFilter;
use crate::structures::document::Document;

/// Composable post-chunking transformation and optimization pipeline.
///
/// Chains quality filtering, deduplication, chunk bin-packing, and metadata enrichment.
#[derive(Debug, Clone, Default)]
pub struct ChunkPipeline {
    filter: Option<ChunkFilter>,
    deduplicator: Option<ChunkDeduplicator>,
    packer: Option<ChunkPacker>,
    enricher: Option<MetadataEnricher>,
}

impl ChunkPipeline {
    /// Create a new, empty ChunkPipeline
    pub fn new() -> Self {
        Self::default()
    }

    /// Set an explicit ChunkFilter stage
    pub fn with_filter(mut self, filter: ChunkFilter) -> Self {
        self.filter = Some(filter);
        self
    }

    /// Set an explicit ChunkDeduplicator stage
    pub fn with_deduplicator(mut self, dedup: ChunkDeduplicator) -> Self {
        self.deduplicator = Some(dedup);
        self
    }

    /// Set an explicit ChunkPacker stage
    pub fn with_packer(mut self, packer: ChunkPacker) -> Self {
        self.packer = Some(packer);
        self
    }

    /// Set an explicit MetadataEnricher stage
    pub fn with_enricher(mut self, enricher: MetadataEnricher) -> Self {
        self.enricher = Some(enricher);
        self
    }

    // --- Fluent Convenience Builders ---

    /// Filter chunks by minimum character count
    pub fn filter_min_characters(mut self, min_chars: usize) -> Self {
        let f = self.filter.take().unwrap_or_default().with_min_characters(min_chars);
        self.filter = Some(f);
        self
    }

    /// Filter chunks by maximum character count
    pub fn filter_max_characters(mut self, max_chars: usize) -> Self {
        let f = self.filter.take().unwrap_or_default().with_max_characters(max_chars);
        self.filter = Some(f);
        self
    }

    /// Filter chunks by minimum word count
    pub fn filter_min_words(mut self, min_words: usize) -> Self {
        let f = self.filter.take().unwrap_or_default().with_min_words(min_words);
        self.filter = Some(f);
        self
    }

    /// Filter chunks by minimum alphanumeric ratio (e.g. 0.5 for 50% letters/digits)
    pub fn filter_min_alpha_ratio(mut self, ratio: f32) -> Self {
        let f = self.filter.take().unwrap_or_default().with_min_alpha_ratio(ratio);
        self.filter = Some(f);
        self
    }

    /// Enable exact deduplication
    pub fn deduplicate_exact(mut self, case_sensitive: bool) -> Self {
        let d = self
            .deduplicator
            .take()
            .unwrap_or_default()
            .with_exact(true)
            .with_case_sensitive(case_sensitive);
        self.deduplicator = Some(d);
        self
    }

    /// Enable normalized (whitespace-collapsed) deduplication
    pub fn deduplicate_normalized(mut self, case_sensitive: bool) -> Self {
        let d = self
            .deduplicator
            .take()
            .unwrap_or_default()
            .with_exact(false)
            .with_case_sensitive(case_sensitive);
        self.deduplicator = Some(d);
        self
    }

    /// Enable chunk bin-packing up to the specified character budget
    pub fn pack(mut self, max_characters: usize) -> Self {
        self.packer = Some(ChunkPacker::new(max_characters));
        self
    }

    /// Enable metadata enrichment (SHA-256 hash, metrics, chunk IDs)
    pub fn enrich_metadata(mut self) -> Self {
        let e = self.enricher.take().unwrap_or_default();
        self.enricher = Some(e);
        self
    }

    /// Set chunk ID prefix for metadata enrichment
    pub fn with_id_prefix(mut self, prefix: impl Into<String>) -> Self {
        let e = self.enricher.take().unwrap_or_default().with_id_prefix(prefix);
        self.enricher = Some(e);
        self
    }

    /// Execute the complete post-chunking pipeline sequentially
    pub fn process(&self, mut docs: Vec<Document>) -> Vec<Document> {
        // 1. Quality Filter
        if let Some(ref filter) = self.filter {
            docs = filter.filter(&docs);
        }

        // 2. Deduplication
        if let Some(ref dedup) = self.deduplicator {
            docs = dedup.deduplicate(&docs);
        }

        // 3. Bin-Packing
        if let Some(ref packer) = self.packer {
            docs = packer.pack(&docs);
        }

        // 4. Metadata Enrichment
        if let Some(ref enricher) = self.enricher {
            docs = enricher.enrich(&docs);
        }

        docs
    }

    /// Execute the complete post-chunking pipeline in parallel across CPU cores
    pub fn par_process(&self, mut docs: Vec<Document>) -> Vec<Document> {
        // 1. Quality Filter
        if let Some(ref filter) = self.filter {
            docs = filter.filter(&docs);
        }

        // 2. Deduplication (inherently sequential order preservation)
        if let Some(ref dedup) = self.deduplicator {
            docs = dedup.deduplicate(&docs);
        }

        // 3. Bin-Packing
        if let Some(ref packer) = self.packer {
            docs = packer.pack(&docs);
        }

        // 4. Metadata Enrichment (parallel)
        if let Some(ref enricher) = self.enricher {
            docs = enricher.par_enrich(&docs);
        }

        docs
    }
}
