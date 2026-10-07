#!/usr/bin/env python3
"""Tune chunker accuracy on the Chroma token-level benchmark and compare frontiers.

Sweeps chunk size, overlap and separator sets for chunkr and the libraries it is compared
against in the README, then reports each implementation's best config at each recall level.
Use it to pick an accuracy-oriented configuration, or to check whether a library's default
settings are hiding a better operating point.

    python benchmarks/tune_accuracy.py                    # default grid, all libraries
    python benchmarks/tune_accuracy.py --libs chunkr      # only chunkr
    python benchmarks/tune_accuracy.py --sizes 800 1000 1200

The metric is the same as `bench_accuracy.py`; see that file for definitions.
"""

from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

import bench_accuracy as bench

DEFAULT_DATA_DIR = Path(__file__).resolve().parent / "data" / "chunking_eval"
RESULTS_DIR = Path(__file__).resolve().parent / "results"

# Sentence-aware hierarchy: keep paragraph and line breaks first, then sentences, then words.
SENT = ["\n\n", "\n", ". ", "! ", "? ", " ", ""]
DEFAULT = ["\n\n", "\n", " ", ""]


def build_grid(libs: list[str], sizes: list[int]) -> list[bench.Impl]:
    impls: list[bench.Impl] = []
    if "chunkr" in libs:
        import chunkr
        for s in sizes:
            impls.append(bench.Impl(f"chunkr ({s},{s // 5}) default", "chunkr", bench.call("chunk", chunkr.RecursiveChunker(s, s // 5, DEFAULT))))
            impls.append(bench.Impl(f"chunkr ({s},0) default", "chunkr", bench.call("chunk", chunkr.RecursiveChunker(s, 0, DEFAULT))))
            impls.append(bench.Impl(f"chunkr ({s},0) sentence", "chunkr", bench.call("chunk", chunkr.RecursiveChunker(s, 0, SENT))))
            impls.append(bench.Impl(f"chunkr ({s},{s // 7}) sentence", "chunkr", bench.call("chunk", chunkr.RecursiveChunker(s, s // 7, SENT))))
    if "chonkie" in libs:
        import chonkie
        for s in sizes:
            impls.append(bench.Impl(f"chonkie ({s})", "chonkie", bench.call("chunk", chonkie.RecursiveChunker(tokenizer="character", chunk_size=s))))
    if "langchain" in libs:
        from langchain_text_splitters import RecursiveCharacterTextSplitter
        for s in sizes:
            impls.append(bench.Impl(f"langchain ({s},{s // 5}) default", "langchain", bench.call("split_text", RecursiveCharacterTextSplitter(chunk_size=s, chunk_overlap=s // 5))))
            impls.append(bench.Impl(f"langchain ({s},0) sentence", "langchain", bench.call("split_text", RecursiveCharacterTextSplitter(separators=SENT, chunk_size=s, chunk_overlap=0))))
    if "semchunk" in libs:
        import semchunk
        for s in sizes:
            impls.append(bench.Impl(f"semchunk ({s},{s // 5})", "semchunk", lambda t, s=s: semchunk.chunk(t, chunk_size=s, token_counter=len, overlap=s // 5)))
    if "text-splitter" in libs:
        import semantic_text_splitter
        for s in sizes:
            impls.append(bench.Impl(f"text-splitter ({s},{s // 5})", "text-splitter", bench.call("chunks", semantic_text_splitter.TextSplitter(s, s // 5))))
    return impls


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--data-dir", type=Path, default=DEFAULT_DATA_DIR)
    parser.add_argument("--libs", default="chunkr,chonkie,langchain,semchunk,text-splitter")
    parser.add_argument("--sizes", type=int, nargs="+", default=[800, 1000, 1200])
    parser.add_argument("--k", type=int, default=5)
    parser.add_argument("--embed-model", default=bench.DEFAULT_MODEL)
    parser.add_argument("--json", action="store_true", help="also dump per-question scores")
    args = parser.parse_args()

    from sentence_transformers import SentenceTransformer

    bench.ensure_dataset(args.data_dir)
    dataset = bench.load_dataset(args.data_dir, None)
    model = SentenceTransformer(args.embed_model)
    corpus_chars = sum(len(t) for t in dataset.corpora.values())

    rows: list[dict] = []
    for impl in build_grid([s.strip() for s in args.libs.split(",")], args.sizes):
        print(f"  running {impl.name} ...", file=sys.stderr)
        result = bench.rank(model, dataset, impl, args.k, scoped=False)
        n = sum(d["n_chunks"] for d in result["diagnostics"].values())
        rows.append(
            {
                "impl": impl.name,
                "lib": impl.lib,
                **result["overall"],
                "n_chunks": n,
                "avg_chunk_chars": round(sum(d["avg_chunk_chars"] * d["n_chunks"] for d in result["diagnostics"].values()) / max(n, 1)),
                "stride": round(corpus_chars / max(n, 1)),
                "unlocated": sum(d["unlocated_chunks"] for d in result["diagnostics"].values()),
                "per_question": result["per_question"] if args.json else None,
            }
        )

    print(f"\n{'config':<36} {'recall':>7} {'prec':>7} {'IoU':>7} {'prec_Ω':>8} {'chunks':>7} {'avg':>6} {'stride':>6}")
    for r in sorted(rows, key=lambda x: (x["lib"], -x["recall"])):
        print(
            f"{r['impl']:<36} {r['recall']:>7.4f} {r['precision']:>7.4f} {r['iou']:>7.4f} "
            f"{r['precision_omega']:>8.4f} {r['n_chunks']:>7} {r['avg_chunk_chars']:>6} {r['stride']:>6}"
        )

    # At each recall level, which library/config gives the cleanest chunks?
    print("\nBest config per recall floor (highest prec_Ω that still reaches the recall):")
    print(f"  {'recall>=':>9} {'config':<36} {'recall':>7} {'prec_Ω':>8} {'IoU':>7}")
    for floor in [0.80, 0.78, 0.76, 0.74, 0.72]:
        eligible = [r for r in rows if r["recall"] >= floor]
        if not eligible:
            continue
        best = max(eligible, key=lambda r: r["precision_omega"])
        print(f"  {floor:>9.2f} {best['impl']:<36} {best['recall']:>7.4f} {best['precision_omega']:>8.4f} {best['iou']:>7.4f}")

    stamp = datetime.now(timezone.utc).strftime("%Y%m%d-%H%M%S")
    RESULTS_DIR.mkdir(parents=True, exist_ok=True)
    out = RESULTS_DIR / f"tuning-{stamp}.json"
    out.write_text(json.dumps({"meta": {"timestamp": stamp, "k": args.k, "embed_model": args.embed_model, "sizes": args.sizes}, "rows": rows}, indent=1))
    print(f"\nwrote {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
