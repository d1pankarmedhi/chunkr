#!/usr/bin/env python3
"""OCR accuracy for the chunkr OCR backends, measured against real ground truth.

A digital page is rasterised with liteparse (so the image has no text layer) and
the backend's OCR is compared with the page's own extracted text. That gives a
reproducible accuracy number without shipping scanned sample files.

    python benchmarks/ocr_accuracy.py                     # fixtures, installed engines
    python benchmarks/ocr_accuracy.py --pages 5 --dpi 200
    python benchmarks/ocr_accuracy.py --markdown          # table for README pastes

Metrics per engine: `char_sim` (difflib similarity of normalised text) and
`token_recall` (share of the page's alphanumeric tokens the engine recovered),
plus mean latency per page. Engines are whatever is installed; Tesseract comes
from liteparse itself, the rest from `chunkr_pdf.OCR_ADAPTERS`.
"""

from __future__ import annotations

import argparse
import difflib
import re
import statistics
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
FIXTURE_DIR = REPO / "tests" / "test_files"
DEFAULT_PDFS = [FIXTURE_DIR / "finance.pdf", FIXTURE_DIR / "lebs201.pdf"]


def tokenize(text: str) -> list[str]:
    return re.findall(r"[0-9A-Za-z]+", text.lower())


def score(reference: str, candidate: str) -> tuple[float, float]:
    """(`char_sim`, `token_recall`) between ground truth and OCR output."""
    reference_norm = " ".join(reference.split())
    candidate_norm = " ".join(candidate.split())
    if not reference_norm:
        return 1.0, 1.0
    similarity = difflib.SequenceMatcher(None, reference_norm, candidate_norm).ratio()
    expected = tokenize(reference_norm)
    if not expected:
        return similarity, 1.0
    got = set(tokenize(candidate_norm))
    recall = sum(1 for token in expected if token in got) / len(expected)
    return similarity, recall


def rasterize(pdf: Path, pages: int, dpi: float) -> list[bytes]:
    """PNG per page, rendered by liteparse: no text layer, exactly what OCR sees."""
    import liteparse

    parser = liteparse.LiteParse(
        extract_screenshots=True, dpi=dpi, ocr_enabled=False, quiet=True
    )
    result = parser.parse(str(pdf))
    return [shot.image_bytes for shot in result.screenshots[:pages]]


def ground_truth(pdf: Path, pages: int) -> list[str]:
    """The pages' own text, extracted digitally (liteparse, no OCR)."""
    from chunkr_pdf import PDFParser

    documents = PDFParser(output="markdown", ocr={"mode": "off"}).load_pages(pdf)
    return [document.content for document in documents[:pages]]


def engines(only: list[str] | None) -> list[str]:
    import chunkr_pdf

    available = ["tesseract"]
    available += [name for name in chunkr_pdf.ocr_backends() if name in chunkr_pdf.OCR_ADAPTERS]
    # Surya needs a llama.cpp/vLLM backend rather than local inference.
    available = [name for name in available if name != "surya"]
    if only:
        available = [name for name in available if name in only]
    return available


def parse_page(parser, png: bytes) -> str:
    documents = parser.load_pages(png)
    return "\n".join(document.content for document in documents)


def run(engine: str, images: list[bytes], reference: list[str], dpi: float) -> dict:
    from chunkr_pdf import PDFParser

    if engine == "tesseract":
        parser = PDFParser(output="markdown", ocr={"mode": "always", "backend": "tesseract"})
    else:
        parser = PDFParser(output="markdown", ocr={"mode": "always", "backend": engine})

    similarities: list[float] = []
    recalls: list[float] = []
    latencies: list[float] = []
    try:
        for png, expected in zip(images, reference):
            start = time.perf_counter()
            text = parse_page(parser, png)
            latencies.append(time.perf_counter() - start)
            similarity, recall = score(expected, text)
            similarities.append(similarity)
            recalls.append(recall)
    finally:
        parser.close()
    return {
        "engine": engine,
        "pages": len(latencies),
        "char_sim": statistics.mean(similarities) if similarities else 0.0,
        "token_recall": statistics.mean(recalls) if recalls else 0.0,
        "ms_per_page": 1000 * statistics.mean(latencies) if latencies else 0.0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("pdfs", nargs="*", type=Path, help="PDFs to use (default: fixtures)")
    parser.add_argument("--pages", type=int, default=3, help="pages per PDF (default 3)")
    parser.add_argument("--dpi", type=float, default=200.0, help="render DPI (default 200)")
    parser.add_argument("--engine", action="append", help="limit to this engine (repeatable)")
    parser.add_argument("--markdown", action="store_true", help="print a markdown table only")
    args = parser.parse_args()

    pdfs = [path for path in (args.pdfs or DEFAULT_PDFS) if path.exists()]
    if not pdfs:
        print("no PDFs found; pass paths or place finance.pdf/lebs201.pdf in tests/test_files")
        return 1

    available = engines(args.engine)
    if not available:
        print("no OCR engine available: pip install 'chunkr-pdf[ocr-rapid]' or use tesseract")
        return 1
    if not args.markdown:
        print(f"# OCR accuracy (ground truth = the pages' own text layer)")
        print(f"# pages per PDF: {args.pages}, render: {args.dpi:g} dpi")
        print(f"# engines: {', '.join(available)}")

    rows = []
    for pdf in pdfs:
        images = rasterize(pdf, args.pages, args.dpi)
        reference = ground_truth(pdf, args.pages)
        if len(images) != len(reference):
            print(f"{pdf.name}: rendered {len(images)} pages, read {len(reference)} — skipped")
            continue
        for engine in available:
            result = run(engine, images, reference, args.dpi)
            result["pdf"] = pdf.name
            rows.append(result)
            if not args.markdown:
                print(
                    f"{pdf.name:16} {engine:10} pages={result['pages']} "
                    f"char_sim={result['char_sim']:.3f} "
                    f"token_recall={result['token_recall']:.3f} "
                    f"{result['ms_per_page']:.0f} ms/page"
                )

    if args.markdown:
        print("| PDF | Engine | Pages | char_sim | token_recall | ms/page |")
        print("| :--- | :--- | ---: | ---: | ---: | ---: |")
        for row in rows:
            print(
                f"| {row['pdf']} | `{row['engine']}` | {row['pages']} | "
                f"{row['char_sim']:.3f} | {row['token_recall']:.3f} | {row['ms_per_page']:.0f} |"
            )
    return 0


if __name__ == "__main__":
    sys.exit(main())
