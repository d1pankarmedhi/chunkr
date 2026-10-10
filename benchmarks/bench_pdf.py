#!/usr/bin/env python3
"""PDF extraction benchmark: chunkr PDFLoader vs pypdf vs PyMuPDF (+ end-to-end chunking).

Same methodology as bench_chunking.py: round-robin reps, median reported, construction
outside the timed region. The large corpus is optional - any *.pdf > 1 MB in the repo
root is used if present.

Run:  .venv/bin/python benchmarks/bench_pdf.py
"""

from __future__ import annotations

import argparse
import gc
import json
import statistics
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(HERE))

from bench_chunking import MB, env_meta, timed_ms  # noqa: E402

SAMPLE = ROOT / "tests" / "test_files" / "sample_doc.pdf"


def large_pdf() -> Path | None:
    candidates = sorted((p for p in ROOT.glob("*.pdf") if p.stat().st_size > 1 * MB), key=lambda p: p.stat().st_size)
    return candidates[-1] if candidates else None


def page_count(path: Path) -> int:
    import pypdf

    return len(pypdf.PdfReader(str(path)).pages)


def build_impls(reps: int) -> dict[str, Callable[[Path], Any]]:
    import fitz
    import pypdf
    import chunkr
    from langchain_text_splitters import RecursiveCharacterTextSplitter

    loader = chunkr.PDFLoader()  # backend="auto"
    fast = chunkr.PDFLoader(backend="fast")  # built-in lopdf extractor, no extra needed
    recursive = chunkr.RecursiveChunker(1000, 200)
    lc = RecursiveCharacterTextSplitter(chunk_size=1000, chunk_overlap=200)
    def pypdf_text(path: Path) -> str:
        reader = pypdf.PdfReader(str(path))
        return "\n".join((page.extract_text() or "") for page in reader.pages)

    def fitz_text(path: Path) -> str:
        doc = fitz.open(str(path))
        try:
            return "\n".join(page.get_text() for page in doc)
        finally:
            doc.close()

    # The high-fidelity backend is optional: `chunkr-rs[pdf]` (chunkr-pdf).
    high = None
    try:
        candidate = chunkr.PDFLoader(backend="liteparse")
        candidate.load_pages(str(SAMPLE))
        high = candidate
    except Exception:
        pass

    impls = {
        # --- extraction only -------------------------------------------------------
        # `auto` resolves to liteparse when the `pdf` extra is installed and to the
        # built-in lopdf extractor otherwise; `fast` is always lopdf.
        "chunkr PDFLoader.load (auto)": lambda p: loader.load(str(p)),
        "chunkr PDFLoader.load_pages (auto)": lambda p: loader.load_pages(str(p)),
        "chunkr PDFLoader.load (fast/lopdf)": lambda p: fast.load(str(p)),
        "chunkr PDFLoader.load_pages (fast/lopdf)": lambda p: fast.load_pages(str(p)),
        "pypdf PdfReader+extract_text": pypdf_text,
        "PyMuPDF (fitz) get_text": fitz_text,
        # --- extraction + recursive chunking (end-to-end) ---------------------------
        "chunkr load (auto) + RecursiveChunker": lambda p: recursive.chunk(loader.load(str(p))),
        "chunkr load (fast) + RecursiveChunker": lambda p: recursive.chunk(fast.load(str(p))),
        "pypdf + langchain RecursiveCharacterTextSplitter": lambda p: lc.split_text(pypdf_text(p)),
        "PyMuPDF + langchain RecursiveCharacterTextSplitter": lambda p: lc.split_text(fitz_text(p)),
    }
    if high is not None:
        impls.update(
            {
                "chunkr [pdf] PDFLoader.load (liteparse)": lambda p: high.load(str(p)),
                "chunkr [pdf] PDFLoader.load_pages (liteparse)": lambda p: high.load_pages(str(p)),
                "chunkr [pdf] load (liteparse) + RecursiveChunker": lambda p: recursive.chunk(high.load(str(p))),
            }
        )
    return impls


def run(impls: dict[str, Callable[[Path], Any]], path: Path, pages: int, size_mb: float, reps: int, warmup: int) -> list[dict[str, Any]]:
    for name, fn in impls.items():
        for _ in range(warmup):
            fn(path)

    inner = {name: max(1, min(50, int(20.0 / max(min(timed_ms(fn, path) for _ in range(3)), 1e-3)))) for name, fn in impls.items()}

    times: dict[str, list[float]] = {name: [] for name in impls}
    outputs: dict[str, Any] = {}
    for _ in range(reps):
        for name, fn in impls.items():
            n = inner[name]
            t0 = time.perf_counter_ns()
            out = None
            for _ in range(n):
                out = fn(path)
            times[name].append((time.perf_counter_ns() - t0) / 1e6 / n)
            outputs.setdefault(name, out)

    rows = []
    for name, ts in times.items():
        med = statistics.median(ts)
        rows.append(
            {
                "impl": name,
                "median_ms": round(med, 3),
                "pages_per_s": round(pages / (med / 1000), 1),
                "mbps": round(size_mb / (med / 1000), 1),
                "spread_pct": round((max(ts) - min(ts)) / med * 100, 1),
                "inner_calls": inner[name],
                "reps_ms": [round(t, 3) for t in ts],
                "chars_out": len(extract_text(outputs[name])),
            }
        )
    return sorted(rows, key=lambda r: r["median_ms"])


def extract_text(out: Any) -> str:
    if isinstance(out, str):
        return out
    parts = []
    for item in out:
        if isinstance(item, str):
            parts.append(item)
        elif hasattr(item, "content"):
            parts.append(item.content)
        elif hasattr(item, "text"):
            parts.append(item.text)
    return "\n".join(parts)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--reps", type=int, default=3)
    ap.add_argument("--warmup", type=int, default=1)
    ap.add_argument("--big", action="store_true", help="include the large PDF (slow: pure-Python extractors take ~20 s/pass)")
    ap.add_argument("--outdir", type=Path, default=HERE / "results")
    args = ap.parse_args()

    corpora = [("sample_doc (10 pages)", SAMPLE)]
    if args.big:
        big = large_pdf()
        if big:
            corpora.append((big.name, big))
        else:
            print("no PDF > 1 MB in repo root - skipping large corpus", file=sys.stderr)

    meta = env_meta()
    payload: dict[str, Any] = {"meta": meta, "config": {"reps": args.reps, "warmup": args.warmup}, "cases": []}
    print(f"machine: {meta['machine']} ({meta['cpu_count']} threads) | python {meta['python']}")

    gc.disable()
    try:
        for label, path in corpora:
            pages = page_count(path)
            size_mb = path.stat().st_size / MB
            impls = build_impls(args.reps)
            rows = run(impls, path, pages, size_mb, args.reps, args.warmup)
            print(f"\n== {label} | {pages} pages | {size_mb:.2f} MB | median of {args.reps} reps")
            for r in rows:
                print(
                    f"   {r['impl']:<52} {r['median_ms']:>9.2f} ms  {r['pages_per_s']:>9.1f} pgs/s  "
                    f"{r['mbps']:>7.1f} MB/s  [spread {r['spread_pct']}%]"
                )
            payload["cases"].append({"label": label, "path": str(path.relative_to(ROOT)), "pages": pages, "mb": round(size_mb, 2), "results": rows})
    finally:
        gc.enable()

    args.outdir.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    json_path = args.outdir / f"pdf-{stamp}.json"
    json_path.write_text(json.dumps(payload, indent=2))
    print(f"\nwrote {json_path.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
