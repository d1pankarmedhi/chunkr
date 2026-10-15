#!/usr/bin/env python3
"""Accuracy benchmark: chunkr vs. the chunking libraries compared in the README.

Dataset (MIT, downloaded and cached, never vendored): Chroma's
"Evaluating Chunking Strategies for Retrieval" -- 5 corpora / 472 questions, each with
gold answer spans (character offsets into the corpus).

Metric: follows Chroma's definitions (all ranges merged, so overlapping chunks count once).
The top-k chunks for a question are retrieved by embedding similarity; the gold spans covered
by those chunks give

    recall        = covered gold chars / all gold chars
    precision     = covered gold chars / chars of retrieved chunks that touch gold
    IoU           = covered / (touching chunk chars + uncovered gold chars)
    precision_omega = same ratio over *all* chunks (perfect-recall precision ceiling,
                      i.e. how "pure" the chunker's chunks are around the answers)

A chunker that cuts through answers scores low on all four; a chunker whose chunks are
far larger than the answers scores well on recall but poorly on precision/IoU.

Usage:
    python benchmarks/bench_accuracy.py                      # all impls, k=5
    python benchmarks/bench_accuracy.py --k 5 10 15          # recall/precision k-sweep
    python benchmarks/bench_accuracy.py --impl chunkr        # only chunkr rows
    python benchmarks/bench_accuracy.py --scoped             # per-corpus retrieval
"""

from __future__ import annotations

import argparse
import json
import platform
import subprocess
import sys
import urllib.request
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable

BENCH_DIR = Path(__file__).resolve().parent
DEFAULT_DATA_DIR = BENCH_DIR / "data" / "chunking_eval"
RESULTS_DIR = BENCH_DIR / "results"

# Pinned to a commit so runs stay reproducible even if upstream changes.
CHROMA_SHA = "e708410d1c61cb76a85cd9d433630ef89b9c6b85"
CHROMA_RAW = (
    "https://raw.githubusercontent.com/brandonstarxel/chunking_evaluation/"
    f"{CHROMA_SHA}/chunking_evaluation/evaluation_framework/general_evaluation_data"
)
CORPORA = ["state_of_the_union", "wikitexts", "chatlogs", "finance", "pubmed"]
DEFAULT_MODEL = "sentence-transformers/all-MiniLM-L6-v2"


# --------------------------------------------------------------------------- dataset


def ensure_dataset(data_dir: Path) -> None:
    """Download the corpora + question file into `data_dir` if not cached."""
    corpora_dir = data_dir / "corpora"
    corpora_dir.mkdir(parents=True, exist_ok=True)
    wanted = {data_dir / "questions_df.csv": f"{CHROMA_RAW}/questions_df.csv"}
    for name in CORPORA:
        wanted[corpora_dir / f"{name}.md"] = f"{CHROMA_RAW}/corpora/{name}.md"
    for path, url in wanted.items():
        if path.exists():
            continue
        print(f"  fetching {path.name} ...", file=sys.stderr)
        with urllib.request.urlopen(url, timeout=60) as resp:
            path.write_bytes(resp.read())


@dataclass
class Dataset:
    corpora: dict[str, str]
    questions: list[dict[str, Any]]  # {question, references: [(start, end)], corpus_id}

    @property
    def n_gold_chars(self) -> int:
        return sum(sum(e - s for s, e in q["references"]) for q in self.questions)


def load_dataset(data_dir: Path, corpus_filter: str | None) -> Dataset:
    import csv

    corpora = {name: (data_dir / "corpora" / f"{name}.md").read_text() for name in CORPORA}
    questions: list[dict[str, Any]] = []
    with open(data_dir / "questions_df.csv", newline="") as fh:
        for row in csv.DictReader(fh):
            refs = json.loads(row["references"])
            questions.append(
                {
                    "question": row["question"],
                    "corpus_id": row["corpus_id"],
                    "references": [(int(r["start_index"]), int(r["end_index"])) for r in refs],
                }
            )
    if corpus_filter:
        corpora = {k: v for k, v in corpora.items() if k == corpus_filter}
        questions = [q for q in questions if q["corpus_id"] in corpora]
    return Dataset(corpora=corpora, questions=questions)


# --------------------------------------------------------------------- char offsets


def locate(text: str, chunks: list[str]) -> tuple[list[tuple[int, int] | None], int]:
    """Character span of each chunk inside `text` (chunks are produced left to right)."""
    spans: list[tuple[int, int] | None] = []
    cursor = 0
    unlocated = 0
    for chunk in chunks:
        start = text.find(chunk, cursor) if chunk else -1
        if start < 0:
            start = text.find(chunk) if chunk else -1  # reordered / repeated chunk
        if start < 0:
            spans.append(None)  # splitter rewrote the text (e.g. header-only splitters)
            unlocated += 1
            continue
        spans.append((start, start + len(chunk)))
        cursor = start + 1
    return spans, unlocated


# -------------------------------------------------------------------- metric helpers


def merge(ranges: list[tuple[int, int]]) -> list[tuple[int, int]]:
    if not ranges:
        return []
    out = [list(sorted(ranges)[0])]
    for start, end in sorted(ranges)[1:]:
        if start <= out[-1][1]:
            out[-1][1] = max(out[-1][1], end)
        else:
            out.append([start, end])
    return [(s, e) for s, e in out]


def length(ranges: list[tuple[int, int]]) -> int:
    return sum(e - s for s, e in ranges)


def difference(ranges: list[tuple[int, int]], cut: tuple[int, int]) -> list[tuple[int, int]]:
    out: list[tuple[int, int]] = []
    cs, ce = cut
    for start, end in ranges:
        if end <= cs or start >= ce:
            out.append((start, end))
        elif start < cs and end > ce:
            out += [(start, cs), (ce, end)]
        elif start < cs:
            out.append((start, cs))
        elif end > ce:
            out.append((ce, end))
    return [r for r in out if r[0] < r[1]]


def cover(gold: list[tuple[int, int]], chunks: list[tuple[int, int]]) -> tuple[list[tuple[int, int]], list[tuple[int, int]]]:
    """(gold ranges covered by `chunks`, chunk ranges that touch gold)."""
    covered: list[tuple[int, int]] = []
    touching: list[tuple[int, int]] = []
    for cs, ce in chunks:
        hit = False
        for gs, ge in gold:
            start, end = max(cs, gs), min(ce, ge)
            if start < end:
                covered.append((start, end))
                hit = True
        if hit:
            touching.append((cs, ce))
    return merge(covered), merge(touching)


def question_scores(
    gold: list[tuple[int, int]],
    retrieved: list[tuple[int, int]],
    everything: list[tuple[int, int]],
) -> dict[str, float]:
    """Token-level precision/recall/IoU of the top-k retrieved chunks against the gold spans.

    All ranges are merged first so overlapping chunks are not double counted.
    """
    gold = merge(gold)
    gold_len = length(gold)
    covered = merge(cover(gold, retrieved)[0])
    covered_len = length(covered)
    retrieved_len = length(merge(retrieved))
    unused = gold_len - covered_len

    covered_all, touching_all = cover(gold, everything)
    unused_ranges = gold
    for cut in covered_all:
        unused_ranges = difference(unused_ranges, cut)
    omega_denom = length(merge(touching_all + unused_ranges))

    return {
        "recall": covered_len / gold_len if gold_len else 0.0,
        "precision": covered_len / retrieved_len if retrieved_len else 0.0,
        "iou": covered_len / (retrieved_len + unused) if (retrieved_len + unused) else 0.0,
        "precision_omega": length(covered_all) / omega_denom if omega_denom else 0.0,
    }


# --------------------------------------------------------------------------- impls


@dataclass
class Impl:
    name: str
    lib: str
    fn: Callable[[str], list[str]]
    note: str = ""


def call(method: str, obj: Any) -> Callable[[Any], Any]:
    return getattr(obj, method)


def contents(result: Any) -> list[str]:
    """Flatten a chunker result into a plain list of chunk strings."""
    if not isinstance(result, (list, tuple)):
        result = [result]
    out: list[str] = []
    for item in result:
        if isinstance(item, str):
            out.append(item)
        elif isinstance(item, (list, tuple)):
            out.extend(contents(item))
        elif hasattr(item, "content"):
            out.append(item.content)
        elif hasattr(item, "text"):
            out.append(item.text)
        else:
            out.append(str(item))
    return out


def try_impl(name: str, lib: str, factory: Callable[[], Callable[[str], list[str]]], note: str = "") -> Impl | None:
    try:
        fn = factory()
    except Exception as exc:  # noqa: BLE001 - benchmark survives missing/broken deps
        print(f"    ! {name} unavailable: {type(exc).__name__}: {exc}", file=sys.stderr)
        return None
    return Impl(name=name, lib=lib, fn=fn, note=note)


def build_groups() -> dict[str, list[Impl]]:
    import chunkr
    import chonkie
    import semchunk
    import semantic_text_splitter
    import llama_index.core as llama_doc
    import llama_index.core.node_parser as llama_parser
    from langchain_text_splitters import MarkdownHeaderTextSplitter, RecursiveCharacterTextSplitter

    def nodes(parser: Any) -> Callable[[str], list[str]]:
        return lambda s: contents(parser.get_nodes_from_documents([llama_doc.Document(text=s)]))

    recursive = [
        i
        for i in [
            try_impl("chunkr RecursiveChunker (library defaults)", "chunkr", lambda: call("chunk", chunkr.RecursiveChunker()), "1000 chars / 120 overlap / sentence separators"),
            try_impl("langchain RecursiveCharacterTextSplitter", "langchain", lambda: call("split_text", RecursiveCharacterTextSplitter(chunk_size=1000, chunk_overlap=200))),
            try_impl("chonkie RecursiveChunker", "chonkie", lambda: call("chunk", chonkie.RecursiveChunker(tokenizer="character", chunk_size=1000)), "no overlap parameter"),
            try_impl("llama-index SentenceSplitter", "llamaindex", lambda: nodes(llama_parser.SentenceSplitter(chunk_size=1000, chunk_overlap=200)), "token-budgeted"),
            try_impl("semchunk chunk(token_counter=len)", "semchunk", lambda: lambda s: semchunk.chunk(s, chunk_size=1000, token_counter=len, overlap=200)),
            try_impl("text-splitter TextSplitter", "text-splitter", lambda: call("chunks", semantic_text_splitter.TextSplitter(1000, 200))),
        ]
        if i
    ]

    markdown = [
        i
        for i in [
            try_impl("chunkr MarkdownChunker", "chunkr", lambda: call("chunk", chunkr.MarkdownChunker(1000, 150))),
            try_impl("langchain MarkdownHeaderTextSplitter", "langchain", lambda: call("split_text", MarkdownHeaderTextSplitter(headers_to_split_on=[("#", "h1"), ("##", "h2"), ("###", "h3")])), "header-only, no size budget, strips headers"),
            try_impl("llama-index MarkdownNodeParser", "llamaindex", lambda: nodes(llama_parser.MarkdownNodeParser()), "header-only, no size budget"),
            try_impl("text-splitter MarkdownSplitter", "text-splitter", lambda: call("chunks", semantic_text_splitter.MarkdownSplitter(1000, 150))),
        ]
        if i
    ]
    return {"recursive (others: 1000 chars / 200 overlap)": recursive, "markdown (1000 chars / 150 overlap)": markdown}


# ----------------------------------------------------------------------- evaluation


def embed(model: Any, texts: list[str]) -> Any:
    import numpy as np

    vectors = model.encode(texts, batch_size=64, normalize_embeddings=True, show_progress_bar=False)
    return np.asarray(vectors, dtype=np.float32)


def rank(model: Any, dataset: Dataset, impl: Impl, ks: int | list[int], scoped: bool = False) -> dict[str, Any]:
    """Score `impl` at one k (flat result) or several k values in one embedding pass.

    An int returns that k's metrics at the top level so single-k callers are unaffected;
    a list returns ``by_k`` keyed by k.
    """
    import numpy as np

    single = isinstance(ks, int)
    k_list = [ks] if single else sorted(set(ks))

    chunk_text, chunk_corpus, chunk_span = [], [], []
    diagnostics: dict[str, dict[str, Any]] = {}
    for corpus_id, text in dataset.corpora.items():
        chunks = [c for c in contents(impl.fn(text)) if c]
        spans, unlocated = locate(text, chunks)
        chunk_text += chunks
        chunk_corpus += [corpus_id] * len(chunks)
        chunk_span += [s for s in spans]
        diagnostics[corpus_id] = {
            "n_chunks": len(chunks),
            "unlocated_chunks": unlocated,
            "avg_chunk_chars": round(sum(map(len, chunks)) / len(chunks), 1) if chunks else 0.0,
            "coverage_pct": round(100 * length(merge([s for s in spans if s])) / len(text), 1),
        }

    chunk_vecs = embed(model, chunk_text)
    question_vecs = embed(model, [q["question"] for q in dataset.questions])
    scores = question_vecs @ chunk_vecs.T

    # Rank the chunks once per question and slice the same order for every k.
    all_ids = np.arange(len(chunk_text))
    scoped_ids = {
        cid: np.array([i for i, c in enumerate(chunk_corpus) if c == cid]) for cid in dataset.corpora
    }
    everything_by_corpus = {
        cid: [s for i, s in enumerate(chunk_span) if chunk_corpus[i] == cid and s is not None]
        for cid in dataset.corpora
    }
    ranked: list[Any] = []
    for row, question in enumerate(dataset.questions):
        candidates = scoped_ids[question["corpus_id"]] if scoped else all_ids
        ranked.append(candidates[np.argsort(-scores[row, candidates])])

    def scores_at(k: int) -> tuple[list[dict[str, Any]], dict[str, list[dict[str, Any]]]]:
        per_question: list[dict[str, Any]] = []
        by_corpus: dict[str, list[dict[str, Any]]] = {}
        for row, question in enumerate(dataset.questions):
            corpus_id = question["corpus_id"]
            top = ranked[row][:k]
            retrieved = [chunk_span[i] for i in top if chunk_corpus[i] == corpus_id and chunk_span[i] is not None]
            result = question_scores(question["references"], retrieved, everything_by_corpus[corpus_id])
            result["question"] = question["question"]
            result["corpus_id"] = corpus_id
            per_question.append(result)
            by_corpus.setdefault(corpus_id, []).append(result)
        return per_question, by_corpus

    def summarize(rows: list[dict[str, Any]]) -> dict[str, float]:
        return {
            metric: round(float(np.mean([r[metric] for r in rows])), 4)
            for metric in ("recall", "precision", "iou", "precision_omega")
        }

    by_k = {}
    for k in k_list:
        per_question, by_corpus = scores_at(k)
        by_k[k] = {
            "overall": summarize(per_question),
            "by_corpus": {cid: summarize(rows) for cid, rows in sorted(by_corpus.items())},
            "per_question": per_question,
        }

    out: dict[str, Any] = {"impl": impl.name, "lib": impl.lib, "note": impl.note, "diagnostics": diagnostics}
    if single:
        out.update(by_k[ks])
    else:
        out["by_k"] = {str(k): by_k[k] for k in k_list}
    return out


def metrics_at(result: dict[str, Any], k: int) -> dict[str, Any]:
    """Metrics of a rank() result at k, for single-k (flat) and multi-k results alike."""
    return result["by_k"][str(k)] if "by_k" in result else result


# ------------------------------------------------------------------------- reporting

METRICS = ["recall", "precision", "iou", "precision_omega"]


def print_group(group: str, results: list[dict[str, Any]], ks: list[int]) -> None:
    for k in ks:
        print(f"\n{group} — top-{k}")
        print(f"  {'implementation':<40} {'recall':>7} {'prec':>7} {'IoU':>7} {'prec_Ω':>7} {'chunks':>7} {'chars':>7} {'unloc':>6}")
        for r in sorted(results, key=lambda x: -metrics_at(x, k)["overall"]["iou"]):
            chunks = sum(d["n_chunks"] for d in r["diagnostics"].values())
            chars = sum(d["avg_chunk_chars"] * d["n_chunks"] for d in r["diagnostics"].values()) / max(chunks, 1)
            unlocated = sum(d["unlocated_chunks"] for d in r["diagnostics"].values())
            o = metrics_at(r, k)["overall"]
            print(f"  {r['impl']:<40} {o['recall']:>7.3f} {o['precision']:>7.3f} {o['iou']:>7.3f} {o['precision_omega']:>7.3f} {chunks:>7} {chars:>7.0f} {unlocated:>6}")


def render_markdown(groups: dict[str, list[dict[str, Any]]], meta: dict[str, Any]) -> str:
    ks: list[int] = meta["ks"]
    lines = [
        "# Chunking accuracy (Chroma token-level retrieval metric)",
        "",
        f"- chunkr commit: `{meta['chunkr_commit']}`",
        f"- date: {meta['timestamp']}",
        f"- host: {meta['platform']}, Python {meta['python']}",
        f"- dataset: {meta['dataset']} ({meta['n_questions']} questions, {meta['n_corpora']} corpora, "
        f"k={', '.join(map(str, ks))}, retrieval={'per-corpus' if meta['scoped'] else 'global'})",
        f"- embedding model: `{meta['embed_model']}`",
        "- metrics: mean per question; `prec@k` counts every retrieved chunk, `prec_Ω` is the "
        "perfect-recall precision ceiling (chunk purity), independent of k",
        "",
    ]
    for group, results in groups.items():
        lines += [f"## {group}", ""]
        lines.append(
            "| implementation |" + "".join(f" recall@{k} | prec@{k} |" for k in ks)
            + " prec_Ω | chunks | avg chunk chars | unlocated | note |"
        )
        lines.append("| --- |" + " ---: |" * (2 * len(ks)) + " ---: | ---: | ---: | ---: | --- |")
        ranked = sorted(
            results,
            key=lambda x: (
                -metrics_at(x, ks[0])["overall"]["recall"],
                -metrics_at(x, ks[0])["overall"]["precision"],
            ),
        )
        for r in ranked:
            o = {k: metrics_at(r, k)["overall"] for k in ks}
            chunks = sum(d["n_chunks"] for d in r["diagnostics"].values())
            chars = sum(d["avg_chunk_chars"] * d["n_chunks"] for d in r["diagnostics"].values()) / max(chunks, 1)
            unlocated = sum(d["unlocated_chunks"] for d in r["diagnostics"].values())
            cells = "".join(f" {o[k]['recall']:.3f} | {o[k]['precision']:.3f} |" for k in ks)
            lines.append(
                f"| {r['impl']} |{cells} {o[ks[0]]['precision_omega']:.3f} | {chunks} | {chars:.0f} | {unlocated} | {r['note']} |"
            )
        lines.append("")

        corpora = sorted({cid for r in results for cid in r["diagnostics"]})
        for k in ks:
            lines += [
                f"### {group} — IoU@{k} by corpus",
                "",
                "| implementation | " + " | ".join(corpora) + " |",
                "| --- | " + " | ".join("---:" for _ in corpora) + " |",
            ]
            for r in ranked:
                cells = [
                    f"{metrics_at(r, k)['by_corpus'][c]['iou']:.3f}" if c in metrics_at(r, k)["by_corpus"] else "—"
                    for c in corpora
                ]
                lines.append(f"| {r['impl']} | " + " | ".join(cells) + " |")
            lines.append("")

    lines += [
        "## Method",
        "",
        "Identical in spirit to Chroma's `chunking_evaluation` metrics, computed here without chromadb.",
        "Chunks are embedded, the top-k are retrieved per question (globally across corpora unless",
        "`--scoped`), and the gold answer spans inside those chunks are counted at the character level.",
        "`recall@k` is the share of gold characters covered by the retrieved chunks, `prec@k` the share",
        "of retrieved characters that are gold (all k chunks count, so larger k or larger chunks lower",
        "it), `IoU@k` their Jaccard index, and `prec_Ω` isolates the chunker's own boundaries (no",
        "retrieval involved, perfect-recall chunk purity). All ranges are merged, so overlapping chunks",
        "are counted once. Chunks that cannot be located in the corpus (splitters that rewrite text,",
        "e.g. header-only splitters that strip headers) are excluded and reported in the `unlocated`",
        "column and the JSON diagnostics.",
        "",
    ]
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--data-dir", type=Path, default=DEFAULT_DATA_DIR)
    parser.add_argument("--k", type=int, nargs="+", default=[5], help="chunks retrieved per question; several values give a k-sweep (default 5)")
    parser.add_argument("--embed-model", default=DEFAULT_MODEL)
    parser.add_argument("--impl", help="only run impls whose name contains this string")
    parser.add_argument("--corpus", help="only evaluate one corpus (e.g. finance)")
    parser.add_argument("--scoped", action="store_true", help="restrict retrieval to the question's corpus")
    parser.add_argument("--json", action="store_true", help="also dump per-question scores")
    args = parser.parse_args()
    ks = sorted(set(args.k))

    from sentence_transformers import SentenceTransformer

    ensure_dataset(args.data_dir)
    dataset = load_dataset(args.data_dir, args.corpus)
    print(f"dataset: {len(dataset.questions)} questions, {len(dataset.corpora)} corpora, "
          f"{sum(len(t) for t in dataset.corpora.values())} chars, {dataset.n_gold_chars} gold chars, k={ks}")
    print(f"embedding model: {args.embed_model} (first run downloads it)")
    model = SentenceTransformer(args.embed_model)

    groups: dict[str, list[dict[str, Any]]] = {}
    for group, impls in build_groups().items():
        if args.impl:
            impls = [i for i in impls if args.impl.lower() in i.name.lower()]
        if not impls:
            continue
        print(f"\n== {group} ==")
        results = []
        for impl in impls:
            print(f"  running {impl.name} ...", file=sys.stderr)
            results.append(rank(model, dataset, impl, ks, args.scoped))
        groups[group] = results
        print_group(group, results, ks)

    if not groups:
        print("no implementations matched", file=sys.stderr)
        return 1

    stamp = datetime.now(timezone.utc).strftime("%Y%m%d-%H%M%S")
    def sh(*cmd: str) -> str:
        try:
            return subprocess.check_output(cmd, text=True).strip()
        except Exception:  # noqa: BLE001
            return "unknown"

    meta = {
        "timestamp": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "chunkr_commit": sh("git", "rev-parse", "--short", "HEAD"),
        "platform": f"{platform.system()} {platform.release()}",
        "python": platform.python_version(),
        "dataset": "chroma/evaluating-chunking",
        "dataset_source": CHROMA_RAW,
        "n_questions": len(dataset.questions),
        "n_corpora": len(dataset.corpora),
        "k": ks[0] if len(ks) == 1 else ks,
        "ks": ks,
        "scoped": args.scoped,
        "embed_model": args.embed_model,
    }

    RESULTS_DIR.mkdir(parents=True, exist_ok=True)
    markdown_path = RESULTS_DIR / f"accuracy-{stamp}.md"
    markdown_path.write_text(render_markdown(groups, meta))
    payload = {"meta": meta, "groups": groups}
    if not args.json:
        for results in groups.values():
            for r in results:
                for block in ([r] if "by_k" not in r else r["by_k"].values()):
                    block.pop("per_question", None)
    json_path = RESULTS_DIR / f"accuracy-{stamp}.json"
    json_path.write_text(json.dumps(payload, indent=1))

    print(f"\nwrote {markdown_path} and {json_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
