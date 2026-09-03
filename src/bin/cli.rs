use clap::{Parser, ValueEnum};
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;

use chunkr::prelude::*;

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueEnum)]
enum Strategy {
    Recursive,
    Token,
    Sentence,
    Paragraph,
    Markdown,
    Table,
    Json,
    Html,
    Late,
    Stream,
    Dir,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Jsonl,
    Json,
    Text,
}

#[derive(Parser, Debug)]
#[command(
    name = "chunkr",
    version,
    about = "⚡ Blazingly fast document & text chunking for LLMs, Agents and RAG"
)]
struct Cli {
    /// Input file or directory path (if omitted or '-', reads from STDIN)
    #[arg(value_name = "INPUT")]
    input: Option<String>,

    /// Chunking strategy to apply
    #[arg(short = 's', long = "strategy", value_enum, default_value_t = Strategy::Recursive)]
    strategy: Strategy,

    /// Target chunk size in characters or tokens
    #[arg(short = 'c', long = "chunk-size", default_value_t = 1000)]
    chunk_size: usize,

    /// Chunk overlap in characters or tokens
    #[arg(short = 'o', long = "overlap", default_value_t = 150)]
    overlap: usize,

    /// Output serialization format (jsonl, json, text)
    #[arg(short = 'f', long = "format", value_enum, default_value_t = OutputFormat::Jsonl)]
    format: OutputFormat,

    /// Write output to file instead of STDOUT
    #[arg(long = "out-file")]
    out_file: Option<PathBuf>,

    /// Optional post-processing chunk bin-packing up to specified character budget
    #[arg(long = "pack")]
    pack: Option<usize>,

    /// Filter chunks shorter than this minimum character count
    #[arg(long = "min-chars")]
    min_chars: Option<usize>,

    /// Filter chunks with fewer than this minimum word count
    #[arg(long = "min-words")]
    min_words: Option<usize>,

    /// Filter chunks with lower than this minimum alphanumeric ratio (e.g. 0.5)
    #[arg(long = "min-alpha-ratio")]
    min_alpha_ratio: Option<f32>,

    /// Deduplicate identical chunks
    #[arg(long = "dedup")]
    dedup: bool,

    /// Enrich chunks with SHA-256 hash, text metrics, and chunk IDs
    #[arg(long = "enrich")]
    enrich: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // 1. If Strategy::Dir, run directory loader directly
    let mut chunks = if cli.strategy == Strategy::Dir {
        let dir_path = cli
            .input
            .as_deref()
            .unwrap_or(".");
        let loader = DirectoryLoader::new()
            .with_chunk_size(cli.chunk_size)
            .with_overlap(cli.overlap);
        loader.load_and_chunk(dir_path)?
    } else if cli.strategy == Strategy::Stream {
        let streamer = StreamChunker::new(cli.chunk_size, cli.overlap)?;
        match cli.input.as_deref() {
            Some("-") | None => {
                let stdin = io::stdin();
                let handle = stdin.lock();
                streamer.chunk_reader(handle).collect::<Result<Vec<_>, _>>()?
            }
            Some(path) => {
                streamer.chunk_file(path)?.collect::<Result<Vec<_>, _>>()?
            }
        }
    } else {
        // Read text from file or STDIN
        let (raw_text, is_pdf) = match cli.input.as_deref() {
            Some("-") | None => {
                let mut buffer = String::new();
                io::stdin().read_to_string(&mut buffer)?;
                (buffer, false)
            }
            Some(path) => {
                if path.to_lowercase().ends_with(".pdf") {
                    let loader = PDFLoader::new();
                    let text = loader.load_from_file(path)?;
                    (text, true)
                } else {
                    let text = fs::read_to_string(path)?;
                    (text, false)
                }
            }
        };

        if raw_text.trim().is_empty() {
            eprintln!("Warning: Empty input provided.");
            return Ok(());
        }

        let _ = is_pdf;

        // Apply strategy
        match cli.strategy {
            Strategy::Recursive => {
                let chunker = RecursiveChunker::new()
                    .with_chunk_size(cli.chunk_size)
                    .with_overlap(cli.overlap);
                chunker.chunk(&raw_text)?
            }
            Strategy::Token => {
                let chunker = TokenChunker::with_encoding(cli.chunk_size, cli.overlap, TokenEncoding::Cl100kBase)?;
                chunker.chunk(&raw_text)?
            }
            Strategy::Sentence => {
                let chunker = SentenceChunker::new()
                    .with_max_characters(cli.chunk_size)
                    .with_sentence_overlap(cli.overlap.min(3));
                chunker.chunk(&raw_text)?
            }
            Strategy::Paragraph => {
                let chunker = ParagraphChunker::new()
                    .with_paragraphs_per_chunk((cli.chunk_size / 200).max(1))
                    .with_paragraph_overlap(cli.overlap.min(1));
                chunker.chunk(&raw_text)?
            }
            Strategy::Markdown => {
                let chunker = MarkdownChunker::new()
                    .with_chunk_size(cli.chunk_size)
                    .with_overlap(cli.overlap);
                chunker.chunk(&raw_text)?
            }
            Strategy::Table => {
                let chunker = TableChunker::new()
                    .with_chunk_size(cli.chunk_size)
                    .with_overlap_rows(cli.overlap.min(5));
                chunker.chunk(&raw_text)?
            }
            Strategy::Json => {
                let chunker = JsonChunker::new()
                    .with_max_chunk_size(cli.chunk_size);
                chunker.chunk(&raw_text)?
            }
            Strategy::Html => {
                let chunker = HtmlChunker::new()
                    .with_chunk_size(cli.chunk_size)
                    .with_overlap(cli.overlap);
                chunker.chunk(&raw_text)?
            }
            Strategy::Late => {
                let chunker = LateChunker::new();
                chunker.chunk(&raw_text)?
            }
            Strategy::Stream | Strategy::Dir => unreachable!(),
        }
    };

    // 2. Post-chunking Pipeline & Optimizations
    let mut pipeline = ChunkPipeline::new();

    if let Some(min_c) = cli.min_chars {
        pipeline = pipeline.filter_min_characters(min_c);
    }
    if let Some(min_w) = cli.min_words {
        pipeline = pipeline.filter_min_words(min_w);
    }
    if let Some(ratio) = cli.min_alpha_ratio {
        pipeline = pipeline.filter_min_alpha_ratio(ratio);
    }
    if cli.dedup {
        pipeline = pipeline.deduplicate_exact(true);
    }
    if let Some(pack_size) = cli.pack {
        pipeline = pipeline.pack(pack_size);
    }
    if cli.enrich {
        pipeline = pipeline.enrich_metadata();
    }

    chunks = pipeline.process(chunks);

    // 3. Serialize output
    let output_str = match cli.format {
        OutputFormat::Jsonl => {
            let mut out = String::new();
            for chunk in chunks {
                let json = serde_json::to_string(&chunk)?;
                out.push_str(&json);
                out.push('\n');
            }
            out
        }
        OutputFormat::Json => serde_json::to_string_pretty(&chunks)?,
        OutputFormat::Text => {
            let mut out = String::new();
            for (i, chunk) in chunks.into_iter().enumerate() {
                out.push_str(&format!("--- [CHUNK {}] ---\n{}\n\n", i, chunk.content));
            }
            out
        }
    };

    // 4. Write output to file or STDOUT
    if let Some(out_path) = cli.out_file {
        fs::write(out_path, output_str)?;
    } else {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        handle.write_all(output_str.as_bytes())?;
    }

    Ok(())
}
