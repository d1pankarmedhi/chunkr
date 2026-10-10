#!/usr/bin/env python3
"""Extraction quality: what each PDF extractor preserves, and what it adds.

Reference-free by design. Three independent extractors read the same embedded text
layer, so a word that at least two of them agree on is treated as really present on
the page ("consensus"). Every engine is then scored on:

* consensus recall   - share of the page's agreed words it actually extracts
* precision          - share of its own words the consensus supports (catches glyph
                       mapping bugs, which fabricate words rather than drop them)
* junk / 1k chars    - control characters, U+FFFD, private-use/unmapped glyph ranges,
                       zero-width marks: text no embedder can use, but which still
                       dilutes every chunk it lands in
* chunk health       - the same texts through RecursiveChunker(1000/120) to see what
                       RAG actually receives: chunks polluted by junk, and chunks with
                       no sentence at all (nothing to cite, nothing to retrieve)
* structure          - headings / tables / lists / figures, which only the layout-aware
                       backend can report

    python benchmarks/pdf_quality.py tests/test_files/sample_doc.pdf
    python benchmarks/pdf_quality.py big.pdf --sample 60 --json out.json
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import Counter
from pathlib import Path

WORD = re.compile(r"[A-Za-z][A-Za-z'\u2019-]{1,}")
JUNK = re.compile(
    "[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\ufffd\ue000-\uf8ff\u200b-\u200f\u0300-\u036f]"
)
SURROGATE = re.compile("[\ud800-\udfff]")


def clean(text: str) -> str:
    """Lone surrogates cannot be encoded at all (they break json/utf-8 writes), so
    they are folded into U+FFFD, which the junk metric already counts."""
    return SURROGATE.sub("\ufffd", text) if SURROGATE.search(text) else text
SENTENCE_END = re.compile(r"[.!?]")

ENGINES = ("chunkr fast (lopdf)", "chunkr [pdf] (liteparse)", "PyMuPDF", "pypdf")


def words(text: str) -> list[str]:
    return [w.lower() for w in WORD.findall(text)]


def junk_count(text: str) -> int:
    return len(JUNK.findall(text))


# ── extractors: each returns a list of page texts ──────────────────────────────
def page_texts_chunkr(path: Path, backend: str) -> list[str] | None:
    try:
        import chunkr
    except ImportError:
        return None
    try:
        pages = chunkr.PDFLoader(backend=backend).load_pages(str(path))
        return [clean(d.content) for d in pages]
    except Exception:  # backend not installed / not registered
        return None


def page_texts_pymupdf(path: Path) -> list[str] | None:
    try:
        import fitz
    except ImportError:
        return None
    with fitz.open(str(path)) as doc:
        return [clean(page.get_text()) for page in doc]


def page_texts_pypdf(path: Path) -> list[str] | None:
    try:
        import pypdf
    except ImportError:
        return None
    return [clean(page.extract_text() or "") for page in pypdf.PdfReader(str(path)).pages]


def structure_counts(path: Path, pages: list[int]) -> dict[str, int] | None:
    """Headings/tables/lists/figures reported by the layout-aware backend."""
    try:
        from chunkr_pdf import PDFParser
    except ImportError:
        return None
    try:
        payload = json.loads(PDFParser(preset="structure").payload(str(path)))
    except Exception:
        return None
    counts: Counter[str] = Counter()
    for page in payload.get("pages", []):
        if pages and page.get("page_number") not in pages:
            continue
        for block in page.get("blocks", []):
            kind = block.get("kind")
            if kind:
                counts[kind] += 1
    return dict(counts)


def chunk_health(texts: list[str], size: int, overlap: int) -> dict[str, float]:
    """What the chunker hands to the embedder, per extractor."""
    import chunkr

    chunker = chunkr.RecursiveChunker(size, overlap)
    total = polluted = sentence_less = 0
    junk = chars = 0
    for text in texts:
        if not text.strip():  # the chunkers reject empty input
            continue
        for chunk in chunker.chunk(text):
            body = chunk.content
            total += 1
            chars += len(body)
            j = junk_count(body)
            junk += j
            polluted += j > 0
            sentence_less += not SENTENCE_END.search(body)
    return {
        "chunks": total,
        "junk_chunks_pct": 100 * polluted / total if total else 0.0,
        "sentence_less_pct": 100 * sentence_less / total if total else 0.0,
        "chunk_junk_per_kchar": 1000 * junk / chars if chars else 0.0,
    }


def sample_pages(total: int, want: int) -> list[int]:
    if total <= want:
        return list(range(1, total + 1))
    step = total / want
    return sorted({min(total, int(i * step) + 1) for i in range(want)})


def evaluate(path: Path, want_pages: int, chunk_size: int, overlap: int) -> dict:
    texts: dict[str, list[str]] = {}
    for name, fn in (
        (ENGINES[0], lambda: page_texts_chunkr(path, "fast")),
        (ENGINES[1], lambda: page_texts_chunkr(path, "liteparse")),
        (ENGINES[2], lambda: page_texts_pymupdf(path)),
        (ENGINES[3], lambda: page_texts_pypdf(path)),
    ):
        out = fn()
        if out:
            texts[name] = out

    if len(texts) < 2:
        raise SystemExit("need at least two extractors installed to build a consensus")

    total_pages = max(len(t) for t in texts.values())
    pages = sample_pages(total_pages, want_pages)

    # consensus: words seen by >= 2 independent extractors, per sampled page
    consensus: set[str] = set()
    for page in pages:
        seen: Counter[str] = Counter()
        for pages_text in texts.values():
            if page - 1 < len(pages_text):
                seen.update(set(words(pages_text[page - 1])))
        consensus.update(w for w, n in seen.items() if n >= 2)

    report: dict = {
        "corpus": str(path),
        "pages_analysed": len(pages),
        "pages_total": total_pages,
        "consensus_words": len(consensus),
        "extractors": {},
    }
    for name, pages_text in texts.items():
        sel = [pages_text[p - 1] for p in pages if p - 1 < len(pages_text)]
        toks: list[str] = []
        junk = chars = 0
        garbage_pages = 0
        for text in sel:
            toks.extend(words(text))
            junk += junk_count(text)
            chars += len(text)
            if text.strip() and not words(text):
                garbage_pages += 1
        tok_set = set(toks)
        hits = len(tok_set & consensus)
        entry = {
            "words": len(toks),
            "words_per_kchar": 1000 * len(toks) / chars if chars else 0.0,
            "consensus_recall": hits / len(consensus) if consensus else 0.0,
            "precision": hits / len(tok_set) if tok_set else 0.0,
            "junk_per_kchar": 1000 * junk / chars if chars else 0.0,
            "pages_without_words": garbage_pages,
            **chunk_health(sel, chunk_size, overlap),
        }
        report["extractors"][name] = entry

    structure = structure_counts(path, pages)
    if structure:
        report["structure"] = structure
    return report


def print_report(report: dict) -> None:
    print(
        f"\n{report['corpus']}  "
        f"({report['pages_analysed']}/{report['pages_total']} pages, "
        f"{report['consensus_words']} consensus words)"
    )
    head = (
        f"{'extractor':26} {'words/kchar':>11} {'consensus':>9} {'precision':>9} "
        f"{'junk/kchar':>10} {'junk chunks':>11} {'no sentence':>11} {'empty pages':>11}"
    )
    print(head)
    print("-" * len(head))
    for name, m in report["extractors"].items():
        print(
            f"{name:26} {m['words_per_kchar']:11.1f} {100 * m['consensus_recall']:8.1f}% "
            f"{100 * m['precision']:8.1f}% {m['junk_per_kchar']:10.1f} "
            f"{m['junk_chunks_pct']:10.1f}% {m['sentence_less_pct']:10.1f}% "
            f"{m['pages_without_words']:11}"
        )
    if report.get("structure"):
        counts = ", ".join(f"{k}={v}" for k, v in sorted(report["structure"].items()))
        print(f"layout-aware structure: {counts}")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("pdf", nargs="+", type=Path)
    ap.add_argument("--sample", type=int, default=60, help="pages analysed per PDF")
    ap.add_argument("--chunk-size", type=int, default=1000)
    ap.add_argument("--overlap", type=int, default=120)
    ap.add_argument("--json", type=Path, default=None, help="write raw metrics")
    args = ap.parse_args(argv)

    reports = []
    for path in args.pdf:
        if not path.exists():
            print(f"missing: {path}", file=sys.stderr)
            continue
        report = evaluate(path, args.sample, args.chunk_size, args.overlap)
        reports.append(report)
        print_report(report)
    if args.json:
        args.json.write_text(json.dumps(reports, indent=2))
        print(f"\nwrote {args.json}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
