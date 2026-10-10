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
# Typographic punctuation differs per extractor for the same source text: liteparse
# normalizes U+2019 to an ASCII apostrophe, pypdf/PyMuPDF keep the curly one, so
# "network's" and "network\u2019s" would be counted as two different words and both
# engines would look worse than they are. Fold quotes before tokenizing.
QUOTES = str.maketrans({"\u2019": "'", "\u2018": "'", "\u00b4": "'", "\u201c": '"', "\u201d": '"'})
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
    text = text.translate(QUOTES)
    return [w.lower() for w in WORD.findall(text)]


def junk_count(text: str) -> int:
    return len(JUNK.findall(text))


def page_map(texts: list[str], page_numbers: list[int] | None = None) -> dict[int, str]:
    """Align pages by their real page number.

    Backends are not obliged to return one entry per document page, and they do not
    all number pages the same way (blank pages get skipped, printed page labels differ
    from physical pages), so index-based alignment silently compares different pages.
    chunkr exposes metadata["page_number"]; the other two are index-ordered.
    """
    if page_numbers is None:
        return {i + 1: t for i, t in enumerate(texts)}
    return {n: t for n, t in zip(page_numbers, texts)}


# ── extractors: each returns a list of page texts ──────────────────────────────
def pages_chunkr(path: Path, backend: str) -> tuple[list[int], list[str], int] | None:
    try:
        import chunkr
    except ImportError:
        return None
    try:
        docs = chunkr.PDFLoader(backend=backend).load_pages(str(path))
    except Exception:  # backend not installed / not registered
        return None
    nums = [d.metadata.get("page_number") for d in docs]
    total = (docs[0].metadata.get("total_pages") if docs else None) or len(docs)
    if any(n is None for n in nums):  # backend without page metadata
        nums = list(range(1, len(docs) + 1))
    return nums, [clean(d.content) for d in docs], int(total)


def pages_pymupdf(path: Path) -> tuple[list[int], list[str], int] | None:
    try:
        import fitz
    except ImportError:
        return None
    with fitz.open(str(path)) as doc:
        texts = [clean(page.get_text()) for page in doc]
    return list(range(1, len(texts) + 1)), texts, len(texts)


def pages_pypdf(path: Path) -> tuple[list[int], list[str], int] | None:
    try:
        import pypdf
    except ImportError:
        return None
    texts = [clean(page.extract_text() or "") for page in pypdf.PdfReader(str(path)).pages]
    return list(range(1, len(texts) + 1)), texts, len(texts)


def structure_counts(path: Path, pages: set[int]) -> dict[str, int] | None:
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
    engines: dict[str, tuple[dict[int, str], int]] = {}
    for name, fn in (
        (ENGINES[0], lambda: pages_chunkr(path, "fast")),
        (ENGINES[1], lambda: pages_chunkr(path, "liteparse")),
        (ENGINES[2], lambda: pages_pymupdf(path)),
        (ENGINES[3], lambda: pages_pypdf(path)),
    ):
        out = fn()
        if out:
            nums, texts, total = out
            engines[name] = (page_map(texts, nums), total)

    if len(engines) < 2:
        raise SystemExit("need at least two extractors installed to build a consensus")

    doc_pages = max(total for _, total in engines.values())
    sample = set(sample_pages(doc_pages, want_pages))

    # consensus: words that >= 2 independent extractors put on the same page
    consensus: set[str] = set()
    for page in sample:
        seen: Counter[str] = Counter()
        for pages_map, _ in engines.values():
            if page in pages_map:
                seen.update(set(words(pages_map[page])))
        consensus.update(w for w, n in seen.items() if n >= 2)

    report: dict = {
        "corpus": str(path),
        "pages_total": doc_pages,
        "pages_analysed": len(sample),
        "consensus_words": len(consensus),
        "extractors": {},
    }
    for name, (pages_map, total) in engines.items():
        sel = [pages_map[p] for p in sorted(sample) if p in pages_map]
        toks: list[str] = []
        junk = chars = questions = 0
        garbage_pages = 0
        for text in sel:
            toks.extend(words(text))
            junk += junk_count(text)
            questions += text.count("?")
            chars += len(text)
            if text.strip() and not words(text):
                garbage_pages += 1
        tok_set = set(toks)
        hits = len(tok_set & consensus)
        top_word, top_hits = Counter(toks).most_common(1)[0] if toks else ("", 0)
        report["extractors"][name] = {
            "pages_returned": len(pages_map),
            "pages_extra": len(pages_map) - total,
            "words": len(toks),
            "words_per_kchar": 1000 * len(toks) / chars if chars else 0.0,
            "consensus_recall": hits / len(consensus) if consensus else 0.0,
            "precision": hits / len(tok_set) if tok_set else 0.0,
            "junk_per_kchar": 1000 * junk / chars if chars else 0.0,
            "question_marks_pct": 100 * questions / chars if chars else 0.0,
            "top_word": top_word,
            "top_word_pct": 100 * top_hits / len(toks) if toks else 0.0,
            "pages_without_words": garbage_pages,
            **chunk_health(sel, chunk_size, overlap),
        }

    structure = structure_counts(path, sample)
    if structure:
        report["structure"] = structure
    return report


def print_report(report: dict) -> None:
    print(
        f"\n{report['corpus']}\n"
        f"  {report['pages_analysed']} of {report['pages_total']} pages analysed, "
        f"{report['consensus_words']} consensus words"
    )
    head = (
        f"{'extractor':24} {'pages':>6} {'words/kch':>9} {'consensus':>9} {'precision':>9} "
        f"{'junk/kch':>8} {'? chars':>8} {'top word':>22} {'junk chunks':>11} {'no sentence':>11}"
    )
    print(head)
    print("-" * len(head))
    for name, m in report["extractors"].items():
        top = f"{m['top_word'][:14]} {m['top_word_pct']:.0f}%"
        print(
            f"{name:24} {m['pages_returned']:6} {m['words_per_kchar']:9.1f} "
            f"{100 * m['consensus_recall']:8.1f}% {100 * m['precision']:8.1f}% "
            f"{m['junk_per_kchar']:8.1f} {m['question_marks_pct']:7.1f}% {top:>22} "
            f"{m['junk_chunks_pct']:10.1f}% {m['sentence_less_pct']:10.1f}%"
        )
    if report.get("structure"):
        counts = ", ".join(f"{k}={v}" for k, v in sorted(report["structure"].items()))
        print(f"  layout-aware structure: {counts}")


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
