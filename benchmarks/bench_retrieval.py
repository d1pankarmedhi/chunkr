#!/usr/bin/env python3
"""Retrieval-quality benchmark over Hugging Face datasets.

The question this answers: **given a query whose evidence sits at a known place in a
source file, does the retrieved chunk contain that evidence — and does the chunk's
metadata say where it came from?**

Corpora are real files in formats `chunkr` supports (``.txt``, ``.csv``), materialised
from pinned Hugging Face datasets; queries and gold evidence come from the dataset, not
from a model. Retrieval is BM25 (lexical, no embedding model), so numbers are
deterministic and do not move with a GPU, a model version or a download.

    python benchmarks/bench_retrieval.py                        # all datasets, defaults
    python benchmarks/bench_retrieval.py --datasets squad wtq
    python benchmarks/bench_retrieval.py --splitters chunkr fixed:1000
    python benchmarks/bench_retrieval.py --sample 200 --k 10

Datasets (pinned to the commit each run resolves, recorded in the results file):

| key | Hugging Face | queries | gold evidence |
| :--- | :--- | :--- | :--- |
| `squad` | `rajpurkar/squad` | reading-comprehension questions | character span of the answer inside the article |
| `scifact` | `mteb/scifact` | scientific claims | relevant document ids (BEIR qrels) |
| `nfcorpus` | `mteb/nfcorpus` | medical queries | relevant document ids (BEIR qrels) |
| `wtq` | `TableSenseAI/WikiTableQuestions` | questions over a table | the table and the row holding the answer |

Metrics

* span datasets (`squad`, `wtq`) — `hit@k` is the share of queries whose answer span or
  answer row is present in one of the top-k chunks; `integrity` is the share of answer
  spans that fit in a single chunk at all (a splitter that cuts answers scores low here
  no matter what k is); `MRR@k` rewards finding it early.
* document datasets (`scifact`, `nfcorpus`) — chunk scores are aggregated per document
  (max), then BEIR-style `nDCG@10`, `recall@10`, `MRR@10` over the qrels, so the effect
  of chunking on document retrieval is what is being measured.

Every splitter sees the same file. `chunkr` uses its own loader (one unit per format
unit, then recursive sentence-aware chunking only for text units); `fixed:N` is the
usual naive character window; `whole` is one chunk per file (no chunking) and exists as
a ceiling for recall and a floor for precision.
"""

from __future__ import annotations

import argparse
import csv
import json
import math
import platform
import random
import re
import sys
import time
import urllib.error
import urllib.request
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable, Iterator, Sequence

BENCH_DIR = Path(__file__).resolve().parent
DEFAULT_DATA_DIR = BENCH_DIR / "data" / "retrieval_hf"
RESULTS_DIR = BENCH_DIR / "results"
HF = "https://huggingface.co"
TEXT_EXTS = {".txt", ".md", ".log", ".json", ".html", ".xml", ".rtf", ".ipynb"}


# --------------------------------------------------------------------------- datasets


@dataclass(frozen=True)
class DatasetSpec:
    key: str
    repo: str
    split_row_limit: int  # rows to sample when the dataset dwarfs a bench run


DATASETS: dict[str, DatasetSpec] = {
    "squad": DatasetSpec("squad", "rajpurkar/squad", 600),
    "scifact": DatasetSpec("scifact", "mteb/scifact", 0),
    "nfcorpus": DatasetSpec("nfcorpus", "mteb/nfcorpus", 0),
    "wtq": DatasetSpec("wtq", "TableSenseAI/WikiTableQuestions", 600),
}


def http_get(url: str, retries: int = 3, timeout: int = 120) -> bytes:
    last: Exception | None = None
    for attempt in range(retries):
        try:
            request = urllib.request.Request(url, headers={"User-Agent": "chunkr-bench/1.0"})
            with urllib.request.urlopen(request, timeout=timeout) as response:
                return response.read()
        except (urllib.error.URLError, TimeoutError) as error:  # transient
            last = error
            time.sleep(1.5 * (attempt + 1))
    raise SystemExit(f"download failed: {url}: {last}")


def pinned_revision(repo: str, cache: dict[str, str]) -> str:
    """Resolve the current commit of a dataset repo and remember it."""
    if repo in cache:
        return cache[repo]
    payload = json.loads(http_get(f"{HF}/api/datasets/{repo}"))
    sha = payload.get("sha")
    if not sha:
        raise SystemExit(f"cannot resolve a revision for {repo}")
    cache[repo] = sha
    return sha


def cached_file(data_dir: Path, repo: str, revision: str, remote: str) -> Path:
    """Download `remote` from `repo`@`revision` once, into the bench cache."""
    target = data_dir / "_downloads" / repo.replace("/", "__") / remote
    if target.exists() and target.stat().st_size > 0:
        return target
    target.parent.mkdir(parents=True, exist_ok=True)
    url = f"{HF}/datasets/{repo}/resolve/{revision}/{remote}"
    print(f"  fetching {repo}/{remote}", file=sys.stderr)
    target.write_bytes(http_get(url))
    return target


def read_jsonl(path: Path) -> Iterator[dict[str, Any]]:
    with path.open(encoding="utf-8") as handle:
        for line in handle:
            if line.strip():
                yield json.loads(line)


def parquet_rows(path: Path) -> list[dict[str, Any]]:
    try:
        import pyarrow.parquet as pq
    except ModuleNotFoundError:  # pragma: no cover - environment issue
        raise SystemExit(
            "pyarrow is required for the parquet-backed datasets (squad, wtq):\n"
            "    pip install pyarrow"
        )
    return pq.read_table(path).to_pylist()


# ---------------------------------------------------------------------- materialising


@dataclass
class Query:
    """One evaluation query with the evidence location its answer implies."""

    text: str
    doc: str  # file name inside the docs directory
    spans: list[tuple[int, int]] = field(default_factory=list)  # char spans (text/IR)
    row: list[str] | None = None  # gold table row, as written in the source
    gold_docs: list[str] = field(default_factory=list)  # qrels, for document datasets


def write_docs(docs_dir: Path, name: str, content: str, suffix: str) -> str:
    docs_dir.mkdir(parents=True, exist_ok=True)
    path = docs_dir / f"{name}{suffix}"
    if not path.exists() or path.read_text(encoding="utf-8") != content:
        path.write_text(content, encoding="utf-8")
    return path.name


def slug(text: str) -> str:
    return re.sub(r"[^A-Za-z0-9_.-]+", "_", text)[:120] or "doc"


def materialise_squad(data_dir: Path, spec: DatasetSpec, revision: str, sample: int) -> list[Query]:
    parquet = cached_file(
        data_dir, spec.repo, revision, "plain_text/train-00000-of-00001.parquet"
    )
    rows = parquet_rows(parquet)
    # Group paragraphs by article: a long document is the interesting chunking case.
    articles: dict[str, list[dict[str, Any]]] = {}
    for row in rows:
        articles.setdefault(row["title"], []).append(row)
    titles = sorted(articles)[:sample]
    docs_dir = data_dir / "squad" / "docs"
    queries: list[Query] = []
    for title in titles:
        # SQuAD repeats one paragraph per question; keep each paragraph once so the
        # document reads like the article instead of a corpus of duplicates.
        unique: dict[str, dict[str, Any]] = {}
        for paragraph in articles[title]:
            unique.setdefault(paragraph["context"], paragraph)
        paragraphs = list(unique.values())
        parts: list[str] = []
        offsets: list[int] = []
        cursor = 0
        for paragraph in paragraphs:
            offsets.append(cursor)
            parts.append(paragraph["context"])
            cursor += len(paragraph["context"]) + 2  # "\n\n"
        document = "\n\n".join(parts)
        name = write_docs(docs_dir, slug(title), document, ".txt")
        for paragraph, offset in zip(paragraphs, offsets):
            answers = paragraph["answers"]
            for text, start in zip(answers["text"], answers["answer_start"]):
                start = int(start)
                if not text or paragraph["context"][start : start + len(text)] != text:
                    continue  # SQuAD has a few off-by-a-few annotations
                queries.append(Query(text=paragraph["question"], doc=name,
                                     spans=[(offset + start, offset + start + len(text))]))
                break  # one query per paragraph is enough
    return queries


def materialise_beir(
    data_dir: Path, spec: DatasetSpec, revision: str, _sample: int = 0, split: str = "test"
) -> list[Query]:
    corpus = cached_file(data_dir, spec.repo, revision, "corpus.jsonl")
    query_file = cached_file(data_dir, spec.repo, revision, "queries.jsonl")
    qrels_file = cached_file(data_dir, spec.repo, revision, f"qrels/{split}.jsonl")
    docs_dir = data_dir / spec.key / "docs"
    names: dict[str, str] = {}
    for row in read_jsonl(corpus):
        title = (row.get("title") or "").strip()
        body = row.get("text") or ""
        document = f"{title}\n\n{body}" if title else body
        names[row["_id"]] = write_docs(docs_dir, slug(row["_id"]), document, ".txt")
    wanted: dict[str, list[str]] = {}
    for row in read_jsonl(qrels_file):
        if int(row.get("score", 0)) > 0:
            wanted.setdefault(row["query-id"], []).append(row["corpus-id"])
    queries: list[Query] = []
    for row in read_jsonl(query_file):
        gold = [names[doc] for doc in wanted.get(row["_id"], []) if doc in names]
        if gold:
            queries.append(Query(text=row["text"], doc=gold[0], gold_docs=gold))
    return queries


def materialise_wtq(data_dir: Path, spec: DatasetSpec, revision: str, sample: int) -> list[Query]:
    examples = json.loads(
        cached_file(data_dir, spec.repo, revision, "examples/examples-train.json")
        .read_text(encoding="utf-8")
    )
    tables = sorted({row["context"]["csv"] for row in examples})
    keep = set(tables[:sample])
    docs_dir = data_dir / "wtq" / "docs"

    def fetch(remote: str) -> tuple[str, str | None]:
        name = remote.split("/")[-1]
        try:
            body = http_get(f"{HF}/datasets/{spec.repo}/resolve/{revision}/{remote}", timeout=60)
        except SystemExit:
            return name, None
        write_docs(docs_dir, Path(name).stem, body.decode("utf-8", "replace"), ".csv")
        return name, body.decode("utf-8", "replace")

    with ThreadPoolExecutor(max_workers=12) as pool:
        contents = dict(pool.map(fetch, sorted(keep)))

    queries: list[Query] = []
    for row in examples:
        remote = row["context"]["csv"]
        if remote not in keep:
            continue
        csv_text = contents.get(remote.split("/")[-1])
        if not csv_text:
            continue
        table_rows = list(csv.reader(csv_text.splitlines()))
        target = row["target_value"].strip()
        gold_row = next(
            (cells for cells in table_rows if any(cell.strip() == target for cell in cells)),
            None,
        )
        if gold_row is None or not any(cell.strip() for cell in gold_row):
            continue
        queries.append(Query(text=row["utterance"], doc=Path(remote).stem + ".csv",
                             row=[cell.strip() for cell in gold_row if cell.strip()]))
    return queries


MATERIALISERS: dict[str, Callable[..., list[Query]]] = {
    "squad": materialise_squad,
    "scifact": materialise_beir,
    "nfcorpus": materialise_beir,
    "wtq": materialise_wtq,
}


# ------------------------------------------------------------------------- splitters


@dataclass
class Chunk:
    text: str
    metadata: dict[str, Any]


def split_chunkr(path: Path, size: int, overlap: int) -> list[Chunk]:
    """`chunkr.load_documents`, then recursive chunking for oversized *text* units."""
    import chunkr as chunkr_module

    units = chunkr_module.load_documents(str(path))
    recursive = chunkr_module.RecursiveChunker(size, overlap, chunkr_module.SENTENCE_SEPARATORS)
    chunks: list[Chunk] = []
    for unit in units:
        if path.suffix.lower() in TEXT_EXTS and len(unit.content) > size:
            for piece in recursive.chunk(unit.content):
                chunks.append(Chunk(piece.content, {**unit.metadata, **piece.metadata}))
        else:
            chunks.append(Chunk(unit.content, dict(unit.metadata)))
    return chunks


def split_fixed(path: Path, size: int, overlap: int) -> list[Chunk]:
    text = path.read_text(encoding="utf-8", errors="replace")
    chunks: list[Chunk] = []
    step = max(1, size - overlap)
    for start in range(0, max(len(text), 1), step):
        piece = text[start : start + size]
        if not piece:
            break
        chunks.append(Chunk(piece, {"source": path.name}))
        if start + size >= len(text):
            break
    return chunks or [Chunk("", {"source": path.name})]


def split_whole(path: Path, size: int, overlap: int) -> list[Chunk]:
    return [Chunk(path.read_text(encoding="utf-8", errors="replace"), {"source": path.name})]


def splitters(specs: Sequence[str], size: int, overlap: int) -> dict[str, Callable[[Path], list[Chunk]]]:
    out: dict[str, Callable[[Path], list[Chunk]]] = {}
    for spec in specs:
        if spec == "chunkr":
            out["chunkr"] = lambda path, s=size, o=overlap: split_chunkr(path, s, o)
        elif spec.startswith("chunkr:"):
            window = int(spec.split(":", 1)[1])
            out[spec] = lambda path, w=window, o=overlap: split_chunkr(path, w, o)
        elif spec == "whole":
            out["whole"] = lambda path, s=size, o=overlap: split_whole(path, s, o)
        elif spec.startswith("fixed:"):
            window = int(spec.split(":", 1)[1])
            out[spec] = lambda path, w=window, o=overlap: split_fixed(path, w, o)
        else:
            raise SystemExit(f"unknown splitter: {spec}")
    return out


# ------------------------------------------------------------------------- retrieval


TOKEN = re.compile(r"[a-z0-9]+")


def tokenize(text: str) -> list[str]:
    return TOKEN.findall(text.lower())


class BM25:
    """Okapi BM25 with an inverted index. No model, no download, no numpy."""

    def __init__(self, chunks: list[Chunk], k1: float = 1.2, b: float = 0.75) -> None:
        self.k1, self.b = k1, b
        self.lengths: list[int] = []
        postings: dict[str, list[tuple[int, int]]] = {}
        for index, chunk in enumerate(chunks):
            tokens = tokenize(chunk.text)
            counts = Counter(tokens)
            self.lengths.append(len(tokens))
            for term, freq in counts.items():
                postings.setdefault(term, []).append((index, freq))
        self.n = len(chunks)
        self.avgdl = (sum(self.lengths) / self.n) if self.n else 0.0
        self.postings = postings
        self.idf = {
            term: math.log(1 + (self.n - len(entries) + 0.5) / (len(entries) + 0.5))
            for term, entries in postings.items()
        }

    def scores(self, query: str, candidates: Sequence[int] | None = None) -> list[float]:
        """BM25 scores; `candidates` restricts which chunks may match."""
        terms = tokenize(query)
        out = [0.0] * self.n
        allowed = set(candidates) if candidates is not None else None
        for term in terms:
            entries = self.postings.get(term)
            if not entries:
                continue
            idf = self.idf[term]
            for index, tf in entries:
                if allowed is not None and index not in allowed:
                    continue
                length = self.lengths[index] or 1
                denominator = tf + self.k1 * (1 - self.b + self.b * length / (self.avgdl or 1))
                out[index] += idf * tf * (self.k1 + 1) / denominator
        return out


def rank(scores: list[float], k: int) -> list[int]:
    order = sorted(range(len(scores)), key=lambda index: scores[index], reverse=True)
    return [index for index in order[:k] if scores[index] > 0]


# --------------------------------------------------------------------------- scoring


def char_spans(document: str, chunks: list[Chunk]) -> list[tuple[int, int] | None]:
    """Where each chunk sits in the source file (None when the text was rewritten)."""
    spans: list[tuple[int, int] | None] = []
    cursor = 0
    for chunk in chunks:
        text = chunk.text
        start = document.find(text, cursor) if text else -1
        if start < 0:
            start = document.find(text) if text else -1
        if start < 0:
            spans.append(None)
        else:
            spans.append((start, start + len(text)))
            cursor = start + 1
    return spans


def overlaps(span: tuple[int, int], gold: tuple[int, int]) -> bool:
    return span[0] < gold[1] and gold[0] < span[1]


def _flatten(text: str) -> str:
    """Compare text the way the chunker renders it: one line, Markdown unescaped."""
    return " ".join(text.replace("\\|", "|").split())


def row_present(chunk_text: str, row: list[str]) -> bool:
    """A table row counts as retrieved when all its cells appear in one line."""
    wanted = [_flatten(cell) for cell in row]
    for line in chunk_text.splitlines():
        flat = _flatten(line)
        if all(cell in flat for cell in wanted):
            return True
    return False


@dataclass
class Report:
    hits: dict[int, int] = field(default_factory=lambda: {1: 0, 3: 0, 5: 0, 10: 0})
    rr: float = 0.0
    integrity: int = 0
    queries: int = 0
    ndcg: float = 0.0
    recall: float = 0.0
    chunks: int = 0
    docs: int = 0
    chunk_chars: int = 0


def evaluate_span(
    queries: list[Query], docs: dict[str, list[Chunk]], k: int, docs_dir: Path
) -> Report:
    """Corpus-wide retrieval: every chunk competes for the top-k, as in a RAG index."""
    flat: list[Chunk] = [chunk for chunks in docs.values() for chunk in chunks]
    origin: list[tuple[str, int]] = [
        (name, position) for name, chunks in docs.items() for position in range(len(chunks))
    ]
    report = Report(queries=len(queries), docs=len(docs), chunks=len(flat))
    report.chunk_chars = sum(len(chunk.text) for chunk in flat)
    index = BM25(flat)
    located: dict[str, list[tuple[int, int] | None]] = {}

    for query in queries:
        chunk_list = docs[query.doc]
        if query.doc not in located:
            source = (docs_dir / query.doc).read_text(encoding="utf-8", errors="replace")
            located[query.doc] = char_spans(source, chunk_list)
        spans = located[query.doc]
        # A splitter cuts an answer when a chunk *starts* inside it. Containment
        # would be the wrong test: overlap re-copies text, so a cut answer still
        # appears whole in the neighbouring chunk.
        if query.row is not None:
            # A table row survives when its cells sit on one line of one chunk.
            if any(row_present(chunk.text, query.row) for chunk in chunk_list):
                report.integrity += 1
        else:
            starts = sorted({span[0] for span in spans if span})
            if not any(query.spans[0][0] < start < query.spans[0][1] for start in starts):
                report.integrity += 1

        for position, index_in_flat in enumerate(rank(index.scores(query.text), k), start=1):
            name, within_doc = origin[index_in_flat]
            if name != query.doc:
                continue  # a hit must be the evidence file, not just any chunk
            chunk = flat[index_in_flat]
            if query.row is not None:
                matched = row_present(chunk.text, query.row)
            else:
                span = spans[within_doc]
                matched = bool(span) and any(overlaps(span, gold) for gold in query.spans)
            if matched:
                report.rr += 1.0 / position
                for cut in report.hits:
                    if position <= cut:
                        report.hits[cut] += 1
                break

    report.rr /= max(report.queries, 1)
    return report


def evaluate_docs(queries: list[Query], docs: dict[str, list[Chunk]], k: int = 10) -> Report:
    """Document-level BEIR scoring: chunk scores aggregated per document by max."""
    report = Report(queries=len(queries), docs=len(docs), chunks=sum(len(c) for c in docs.values()))
    report.chunk_chars = sum(len(chunk.text) for chunks in docs.values() for chunk in chunks)
    flat: list[tuple[str, Chunk]] = [
        (name, chunk) for name, chunks in docs.items() for chunk in chunks
    ]
    index = BM25([chunk for _, chunk in flat])
    for query in queries:
        scores = index.scores(query.text)
        best: dict[str, float] = {}
        for (name, _), score in zip(flat, scores):
            if score > best.get(name, -1.0):
                best[name] = score
        order = sorted(best, key=lambda name: best[name], reverse=True)[:k]
        position = order.index(query.doc) + 1 if query.doc in order else 0
        if position:
            report.rr += 1.0 / position
            report.recall += 1.0
            report.ndcg += 1.0 / math.log2(position + 1)
    total = max(report.queries, 1)
    report.rr /= total
    report.recall /= total
    report.ndcg /= total
    return report


# ------------------------------------------------------------------------------ main


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--datasets", nargs="+", default=list(DATASETS), choices=list(DATASETS))
    parser.add_argument(
        "--splitters",
        nargs="+",
        default=["chunkr", "chunkr:2000", "fixed:1000", "fixed:2000", "whole"],
    )
    parser.add_argument("--size", type=int, default=1000, help="chunkr target chunk size in chars")
    parser.add_argument("--overlap", type=int, default=120, help="overlap for every splitter")
    parser.add_argument("--sample", type=int, default=0, help="documents per dataset (0 = default)")
    parser.add_argument("--corpus", type=int, default=0, help="max documents in the index (0 = all)")
    parser.add_argument("--queries", type=int, default=400, help="max queries per dataset")
    parser.add_argument("--k", type=int, default=10)
    parser.add_argument("--data-dir", type=Path, default=DEFAULT_DATA_DIR)
    parser.add_argument("--out", type=Path, default=RESULTS_DIR)
    parser.add_argument("--seed", type=int, default=7)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    random.seed(args.seed)
    data_dir = args.data_dir
    data_dir.mkdir(parents=True, exist_ok=True)
    revision_cache_path = data_dir / "revisions.json"
    revisions: dict[str, str] = (
        json.loads(revision_cache_path.read_text()) if revision_cache_path.exists() else {}
    )

    splitter_fns = splitters(args.splitters, args.size, args.overlap)
    started = time.time()
    results: list[dict[str, Any]] = []
    skipped: list[str] = []

    for key in args.datasets:
        spec = DATASETS[key]
        revision = pinned_revision(spec.repo, revisions)
        revision_cache_path.write_text(json.dumps(revisions, indent=2, sort_keys=True))
        print(f"\n== {key} @ {revision[:10]}", file=sys.stderr)
        sample = args.sample or spec.split_row_limit
        queries = MATERIALISERS[key](data_dir, spec, revision, sample)
        random.shuffle(queries)
        queries = queries[: args.queries]
        docs_dir = data_dir / key / "docs"
        corpus = sorted(path.name for path in docs_dir.iterdir() if path.is_file())
        if args.corpus:
            corpus = corpus[: args.corpus]
        files = sorted({query.doc for query in queries} & set(corpus))
        queries = [query for query in queries if query.doc in set(corpus)]
        print(f"   {len(queries)} queries over {len(files)} files ({len(corpus)} in the index)",
              file=sys.stderr)

        is_document_task = bool(queries and queries[0].gold_docs)

        for name, split in splitter_fns.items():
            per_file: dict[str, list[Chunk]] = {}
            try:
                for file_name in corpus:
                    per_file[file_name] = split(docs_dir / file_name)
            except Exception as error:  # a splitter that cannot read a file is a result
                skipped.append(f"{key}/{name}: {error}")
                print(f"   [{name}] skipped: {error}", file=sys.stderr)
                continue
            if is_document_task:
                report = evaluate_docs(queries, per_file, args.k)
            else:
                report = evaluate_span(queries, per_file, args.k, docs_dir)
            row = {
                "dataset": key,
                "splitter": name,
                "queries": report.queries,
                "documents": report.docs,
                "chunks": report.chunks,
                "chunks_per_doc": report.chunks / max(report.docs, 1),
                "mean_chunk_chars": report.chunk_chars / max(report.chunks, 1),
                "mrr": report.rr,
                "integrity": None if is_document_task else report.integrity / max(report.queries, 1),
            }
            for cut in (1, 3, 5, 10):
                row[f"hit@{cut}"] = (
                    None if is_document_task else report.hits.get(cut, 0) / max(report.queries, 1)
                )
            if is_document_task:
                row.update(ndcg10=report.ndcg, recall10=report.recall)
            results.append(row)
            line = "   ".join(
                f"{field}={row[field]:.3f}"
                for field in ("hit@1", "hit@5", "mrr", "integrity", "ndcg10", "recall10")
                if isinstance(row.get(field), float)
            )
            print(f"   [{name}] {line}  chunks/doc={row['chunks_per_doc']:.1f}", file=sys.stderr)

    payload = {
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "python": platform.python_version(),
        "platform": platform.platform(),
        "settings": vars(args) | {"data_dir": str(args.data_dir), "out": str(args.out)},
        "revisions": {DATASETS[key].repo: revisions.get(DATASETS[key].repo) for key in args.datasets},
        "results": results,
        "skipped": skipped,
        "elapsed_s": round(time.time() - started, 1),
    }
    args.out.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    json_path = args.out / f"retrieval_hf_{stamp}.json"
    json_path.write_text(json.dumps(payload, indent=2))
    report = markdown(payload)
    json_path.with_suffix(".md").write_text(report + "\n")
    print(f"\nwrote {json_path}", file=sys.stderr)
    print(report)
    return 0


def markdown(payload: dict[str, Any]) -> str:
    head = "| dataset | splitter | hit@1 | hit@5 | MRR | integrity | nDCG@10 | R@10 | chunks/doc | mean chars |"
    rule = "| :--- | :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |"
    lines = [head, rule]
    for row in payload["results"]:
        def fmt(key: str, digits: int = 3) -> str:
            value = row.get(key)
            return "—" if value is None else f"{value:.{digits}f}"

        lines.append(
            "| {dataset} | `{splitter}` | {h1} | {h5} | {mrr} | {integ} | {ndcg} | {rec} | "
            "{cpd:.1f} | {chars:.0f} |".format(
                dataset=row["dataset"],
                splitter=row["splitter"],
                h1=fmt("hit@1"),
                h5=fmt("hit@5"),
                mrr=fmt("mrr"),
                integ=fmt("integrity"),
                ndcg=fmt("ndcg10"),
                rec=fmt("recall10"),
                cpd=row["chunks_per_doc"],
                chars=row["mean_chunk_chars"],
            )
        )
    for note in payload["skipped"]:
        lines.append(f"\n_skipped: {note}_")
    return "\n".join(lines)


if __name__ == "__main__":
    raise SystemExit(main())
