use clap::{Parser, Subcommand, ValueEnum};
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

    /// Transcribe audio with a local whisper.cpp build (`--features audio`)
    #[arg(long = "asr", value_name = "MODEL", help_heading = "Audio")]
    asr: Option<String>,

    /// Skip a model download and fail if it is not already cached
    #[arg(long = "offline", help_heading = "Audio")]
    offline: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Manage speech models: list the pinned ones, or download one
    Models {
        #[command(subcommand)]
        action: ModelsAction,
    },
}

#[derive(Subcommand, Debug)]
enum ModelsAction {
    /// Show the pinned models, their size and whether they are downloaded
    List,
    /// Download models (verifying size and SHA-256)
    Pull {
        /// One or more model names (`tiny.en`, `base`, `small.en`)
        #[arg(value_name = "MODEL", required = true)]
        models: Vec<String>,
    },
    /// Print the model cache directory
    Dir,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    if let Some(Command::Models { action }) = cli.command {
        return run_models(action);
    }

    #[cfg(all(feature = "audio", not(target_arch = "wasm32")))]
    if let Some(model) = &cli.asr {
        configure_asr(model, cli.offline)?;
    }
    #[cfg(any(not(feature = "audio"), target_arch = "wasm32"))]
    if cli.asr.is_some() {
        return Err("--asr needs a build with `--features audio`".into());
    }

    // 1. If Strategy::Dir, run directory loader directly
    let mut chunks = if cli.strategy == Strategy::Dir {
        let dir_path = cli.input.as_deref().unwrap_or(".");
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
                streamer
                    .chunk_reader(handle)
                    .collect::<Result<Vec<_>, _>>()?
            }
            Some(path) => streamer.chunk_file(path)?.collect::<Result<Vec<_>, _>>()?,
        }
    } else {
        match cli.input.as_deref() {
            // STDIN is one unnamed unit of text.
            Some("-") | None => {
                let mut buffer = String::new();
                io::stdin().read_to_string(&mut buffer)?;
                if buffer.trim().is_empty() {
                    eprintln!("Warning: Empty input provided.");
                    return Ok(());
                }
                chunk_text(&cli, &buffer)?
            }
            // A file is chunked per extracted unit (page, sheet, slide, chapter,
            // email, archive member, …), so every chunk keeps that unit's
            // metadata instead of the units collapsing into one text blob.
            Some(path) => {
                let documents = load_documents(path)?;
                if documents
                    .iter()
                    .all(|document| document.content.trim().is_empty())
                {
                    eprintln!("Warning: Empty input provided.");
                    return Ok(());
                }
                let mut chunks = Vec::new();
                for document in documents {
                    for mut chunk in chunk_text(&cli, &document.content)? {
                        for (key, value) in &document.metadata {
                            chunk
                                .metadata
                                .entry(key.clone())
                                .or_insert_with(|| value.clone());
                        }
                        chunks.push(chunk);
                    }
                }
                chunks
            }
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
            // The unit a chunk came from (page, sheet, cue span, archive member)
            // is printed whenever it changes, so text output keeps the provenance
            // the JSON metadata already carries.
            let mut current_unit: Option<String> = None;
            for (i, chunk) in chunks.into_iter().enumerate() {
                let unit = unit_label(&chunk.metadata);
                if unit.is_some() && unit != current_unit {
                    if let Some(label) = &unit {
                        out.push_str(&format!("=== {label} ===\n"));
                    }
                }
                current_unit = unit;
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

/// Chunk one unit of text with the selected strategy.
fn chunk_text(cli: &Cli, text: &str) -> Result<Vec<Document>, Box<dyn std::error::Error>> {
    let chunks = match cli.strategy {
        Strategy::Recursive => {
            let chunker = RecursiveChunker::new()
                .with_chunk_size(cli.chunk_size)
                .with_overlap(cli.overlap);
            chunker.chunk(text)?
        }
        Strategy::Token => {
            let chunker = TokenChunker::with_encoding(
                cli.chunk_size,
                cli.overlap,
                TokenEncoding::Cl100kBase,
            )?;
            chunker.chunk(text)?
        }
        Strategy::Sentence => {
            // SentenceChunker defaults to sentences_per_chunk=3 and rejects
            // overlap >= count, so clamp strictly below 3 (was .min(3),
            // which made even default flags fail with InvalidOverlap).
            let chunker = SentenceChunker::new()
                .with_max_characters(cli.chunk_size)
                .with_sentence_overlap(cli.overlap.min(2));
            chunker.chunk(text)?
        }
        Strategy::Paragraph => {
            let ppc = (cli.chunk_size / 200).max(1);
            // Overlap must stay strictly below the paragraph count.
            let chunker = ParagraphChunker::new()
                .with_paragraphs_per_chunk(ppc)
                .with_paragraph_overlap(cli.overlap.min(ppc.saturating_sub(1)));
            chunker.chunk(text)?
        }
        Strategy::Markdown => {
            let chunker = MarkdownChunker::new()
                .with_chunk_size(cli.chunk_size)
                .with_overlap(cli.overlap);
            chunker.chunk(text)?
        }
        Strategy::Table => {
            let chunker = TableChunker::new()
                .with_chunk_size(cli.chunk_size)
                .with_overlap_rows(cli.overlap.min(5));
            chunker.chunk(text)?
        }
        Strategy::Json => {
            let chunker = JsonChunker::new().with_max_chunk_size(cli.chunk_size);
            chunker.chunk(text)?
        }
        Strategy::Html => {
            let chunker = HtmlChunker::new()
                .with_chunk_size(cli.chunk_size)
                .with_overlap(cli.overlap);
            chunker.chunk(text)?
        }
        Strategy::Late => {
            let chunker = LateChunker::new();
            chunker.chunk(text)?
        }
        Strategy::Stream | Strategy::Dir => unreachable!(),
    };
    Ok(chunks)
}

/// Short label for the unit a chunk came from, from its flat metadata.
///
/// Named `<file> · <unit>`; an archive member's `source` already carries the
/// chain (`bundle.zip!data.csv`), so it is the label on its own.
fn unit_label(metadata: &std::collections::HashMap<String, serde_json::Value>) -> Option<String> {
    let text = |key: &str| metadata.get(key).and_then(|value| value.as_str());
    let number = |key: &str| metadata.get(key).and_then(|value| value.as_i64());

    let name = text("file_name")
        .or_else(|| text("source"))
        .or_else(|| text("file_path"));

    let unit = if metadata.contains_key("archive_entry") {
        // `source` is the full chain: archive plus member, which is more
        // specific than the container's file name.
        return text("source").or(name).map(str::to_string);
    } else if let Some(sheet) = text("sheet_name") {
        Some(match (number("sheet_index"), number("total_sheets")) {
            // Stored 0-based; the label is for humans.
            (Some(index), Some(total)) => format!("sheet {} {}/{}", sheet, index + 1, total),
            (Some(index), None) => format!("sheet {} {}", sheet, index + 1),
            _ => format!("sheet {sheet}"),
        })
    } else if let Some(page) = number("page_number") {
        Some(match number("total_pages") {
            Some(total) => format!("page {page}/{total}"),
            None => format!("page {page}"),
        })
    } else if let Some(slide) = number("slide_number") {
        Some(match number("total_slides") {
            Some(total) => format!("slide {slide}/{total}"),
            None => format!("slide {slide}"),
        })
    } else if let Some(chapter) = text("chapter_title").or_else(|| text("chapter_href")) {
        Some(chapter.to_string())
    } else if let Some(start) = metadata
        .get("start_time_s")
        .and_then(|value| value.as_f64())
    {
        Some(
            match metadata.get("end_time_s").and_then(|value| value.as_f64()) {
                Some(end) => format!("{start:.1}s-{end:.1}s"),
                None => format!("{start:.1}s"),
            },
        )
    } else {
        number("start_record").map(|start| {
            number("end_record").map_or_else(
                || format!("record {start}"),
                |end| format!("records {start}-{end}"),
            )
        })
    };

    match (name, unit) {
        (Some(name), Some(unit)) => Some(format!("{name} · {unit}")),
        (Some(name), None) => Some(name.to_string()),
        (None, unit) => unit,
    }
}

// ── Audio: model management and the whisper.cpp bridge ──────────────────────

#[cfg(all(feature = "audio", not(target_arch = "wasm32")))]
fn run_models(action: ModelsAction) -> Result<(), Box<dyn std::error::Error>> {
    use chunkr::audio::models;

    match action {
        ModelsAction::Dir => println!("{}", models::cache_dir().display()),
        ModelsAction::List => {
            let cache = models::cache_dir();
            println!("cache: {}", cache.display());
            println!();
            println!("{:<10} {:>10}  {:<12}  STATUS", "MODEL", "SIZE", "LANGUAGE");
            for spec in models::MODELS {
                let status = if models::cached(spec).is_some() {
                    "downloaded".to_string()
                } else {
                    String::new()
                };
                println!(
                    "{:<10} {:>7.0} MB  {:<10}  {}",
                    spec.name,
                    spec.bytes as f64 / 1e6,
                    if spec.multilingual {
                        "multilingual"
                    } else {
                        "english"
                    },
                    status
                );
            }
        }
        ModelsAction::Pull { models: names } => {
            for name in names {
                let path = models::pull(&name)?;
                println!("{} -> {}", name, path.display());
            }
        }
    }
    Ok(())
}

#[cfg(any(not(feature = "audio"), target_arch = "wasm32"))]
fn run_models(_action: ModelsAction) -> Result<(), Box<dyn std::error::Error>> {
    Err("model management needs a build with `--features audio`".into())
}

/// Resolve the model, find the binary, and register whisper.cpp for this run.
#[cfg(all(feature = "audio", not(target_arch = "wasm32")))]
fn configure_asr(model: &str, offline: bool) -> Result<(), Box<dyn std::error::Error>> {
    use chunkr::audio::models;

    let model_path = if offline {
        models::resolve(model)?
    } else {
        models::pull(model)?
    };
    let binary = models::find_binary().ok_or(
        "no whisper.cpp binary found — install it (`brew install whisper-cpp`) or set \
         CHUNKR_WHISPER_BIN, then run `chunkr models pull base.en`",
    )?;

    chunkr::audio::register_transcriber(
        "whisper-cpp",
        Box::new(chunkr::audio::CommandTranscriber::whisper_cpp(
            binary, model_path,
        )),
    );
    Ok(())
}
