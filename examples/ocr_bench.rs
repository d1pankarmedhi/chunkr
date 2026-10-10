//! OCR accuracy harness for the native backends (Tesseract, PP-OCR).
//!
//! Rasterises pages with liteparse so the images carry no text layer, OCRs each
//! one through `chunkr::parser::PdfParser`, and scores the result against the
//! text liteparse extracts from the same page digitally. Mirrors
//! `benchmarks/ocr_accuracy.py` for the Python engines.
//!
//! ```text
//! cargo run --release --features pdf-ocr-ppocr --example ocr_bench -- \
//!     tests/test_files/textbook_27p.pdf --preset tiny --pages 5
//! cargo run --release --features pdf-ocr --example ocr_bench -- \
//!     tests/test_files/textbook_27p.pdf --tesseract --pages 5
//! ```

use std::path::{Path, PathBuf};
use std::time::Instant;

use chunkr::parser::{Backend, Granularity, OcrMode, ParserConfig, ParserOutput, PdfParser};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut pdf: Option<PathBuf> = None;
    let mut pages = 0usize;
    let mut dpi = 200.0f32;
    let mut preset = "small".to_string();
    let mut tesseract = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--pages" => {
                index += 1;
                pages = args.get(index).ok_or("--pages needs a value")?.parse()?;
            }
            "--dpi" => {
                index += 1;
                dpi = args.get(index).ok_or("--dpi needs a value")?.parse()?;
            }
            "--preset" => {
                index += 1;
                preset = args.get(index).ok_or("--preset needs a value")?.clone();
            }
            "--tesseract" => tesseract = true,
            other => pdf = Some(PathBuf::from(other)),
        }
        index += 1;
    }
    let pdf = pdf.ok_or(
        "usage: ocr_bench <pdf> [--pages N] [--dpi D] [--preset tiny|small|medium] [--tesseract]",
    )?;

    // Ground truth: the page's own text, no OCR involved.
    let base = ParserConfig {
        backend: Backend::Liteparse,
        output: ParserOutput::Markdown,
        granularity: Granularity::Page,
        ocr: chunkr::parser::OcrConfig {
            mode: OcrMode::Off,
            ..Default::default()
        },
        ..Default::default()
    };
    let reference: Vec<String> = PdfParser::new(base)
        .load_pages(&pdf)?
        .into_iter()
        .map(|document| document.content)
        .collect();

    // Rasterise the same pages: this is what the OCR engine sees.
    let images = screenshots(&pdf, dpi, pages)?;
    let page_count = reference.len().min(images.len());
    if page_count == 0 {
        return Err("no pages to compare".into());
    }

    let mut config = ParserConfig {
        backend: Backend::Liteparse,
        output: ParserOutput::Markdown,
        ocr: chunkr::parser::OcrConfig {
            mode: OcrMode::Always,
            backend: if tesseract {
                "tesseract".to_string()
            } else {
                "ppocr".to_string()
            },
            language: "en".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    config.ocr.ppocr.preset = preset.clone();
    let engine_name = if tesseract {
        "tesseract".to_string()
    } else {
        format!("ppocr-{preset}")
    };
    // Page granularity keeps the comparison per page regardless of the caller.
    config.granularity = Granularity::Page;
    let parser = PdfParser::new(config);

    let mut similarities = Vec::new();
    let mut recalls = Vec::new();
    let mut ratios = Vec::new();
    let mut latencies = Vec::new();
    for (page, image) in images.iter().take(page_count).enumerate() {
        let started = Instant::now();
        let documents = parser.load_from_bytes(image, None)?;
        latencies.push(started.elapsed().as_secs_f64() * 1000.0);
        let text = documents
            .iter()
            .map(|document| document.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let (similarity, recall) = score(&reference[page], &text);
        similarities.push(similarity);
        recalls.push(recall);
        let expected_chars = normalise(&reference[page]).chars().count().max(1);
        ratios.push(normalise(&text).chars().count() as f64 / expected_chars as f64);
    }

    let mean = |values: &[f64]| values.iter().sum::<f64>() / values.len() as f64;
    println!(
        "| {} | `{}` | {} | {:.3} | {:.3} | {:.3} | {:.0} |",
        pdf.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("pdf"),
        engine_name,
        page_count,
        mean(&similarities),
        mean(&recalls),
        mean(&ratios),
        mean(&latencies)
    );
    Ok(())
}

/// PNG per page, rendered by liteparse at `dpi` (no text layer in the output).
fn screenshots(
    pdf: &Path,
    dpi: f32,
    pages: usize,
) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error>> {
    let mut config = liteparse::LiteParseConfig {
        extract_screenshots: true,
        dpi,
        ocr_enabled: false,
        quiet: true,
        extract_blocks: false,
        ..Default::default()
    };
    if pages > 0 {
        config.max_pages = pages;
    }
    let parser = liteparse::LiteParse::new(config);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(parser.parse_input(liteparse::types::PdfInput::Path(
        pdf.to_string_lossy().to_string(),
    )))?;
    Ok(result
        .screenshots
        .into_iter()
        .map(|screenshot| screenshot.image_bytes)
        .collect())
}

fn normalise(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn tokens(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_lowercase())
        .collect()
}

/// (`char_sim`, `token_recall`) for one page, matching the Python harness.
fn score(reference: &str, candidate: &str) -> (f64, f64) {
    let reference = normalise(reference);
    let candidate = normalise(candidate);
    if reference.is_empty() {
        return (1.0, 1.0);
    }
    let distance = levenshtein(&reference, &candidate);
    let longest = reference
        .chars()
        .count()
        .max(candidate.chars().count())
        .max(1);
    let similarity = 1.0 - distance as f64 / longest as f64;

    let expected = tokens(&reference);
    if expected.is_empty() {
        return (similarity, 1.0);
    }
    let seen: std::collections::HashSet<String> = tokens(&candidate).into_iter().collect();
    let found = expected
        .iter()
        .filter(|token| seen.contains(*token))
        .count();
    (similarity, found as f64 / expected.len() as f64)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];
    for (i, left) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, right) in b.iter().enumerate() {
            let cost = usize::from(left != right);
            current[j + 1] = (previous[j] + cost)
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}
