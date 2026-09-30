#!/usr/bin/env python3
"""Reproducible chunking throughput benchmark: chunkr vs the mainstream alternatives.

Methodology (see benchmarks/README.md for the full rationale):
  * identical in-memory corpora, generated deterministically (seeds fixed, SHA-256 recorded)
  * identical chunk-size parameters per case; chunk counts and average chunk size are
    reported so unequal work is visible
  * round-robin repetition (one timed call per implementation per round) to cancel
    ordering/thermal drift; median of N reps is reported
  * gc disabled for the whole measurement phase; chunker construction happens outside
    the timed region (construction cost is reported separately)

Run:  .venv/bin/python benchmarks/bench_chunking.py
"""

from __future__ import annotations

import argparse
import gc
import hashlib
import json
import platform
import random
import statistics
import subprocess
import sys
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
MB = 1_000_000  # throughput uses 10^6 bytes per MB


# --------------------------------------------------------------------------------------
# corpora
# --------------------------------------------------------------------------------------

WORDS = (
    "attention embedding retrieval token vector corpus index query latency chunk overlap "
    "context model gradient activation transformer encoder decoder dataset pipeline pruning "
    "quantization baseline hypothesis inference kernel matrix memory neural optimizer "
    "parameter residual sampling sparsity threshold reranker reranking centroid cluster "
    "fragment window stride offset anchor semantic lexical evaluation precision recall"
).split()


def _trim(text: str, n_bytes: int) -> str:
    """Cut `text` to <= n_bytes, preferring the last newline boundary."""
    raw = text.encode()
    if len(raw) <= n_bytes:
        return text
    cut = text[:n_bytes]
    nl = cut.rfind("\n")
    return cut[:nl] if nl > n_bytes // 2 else cut


def make_prose(n_bytes: int, seed: int = 42) -> str:
    """Paragraph/heading/bullet mix: exercises recursive separators realistically."""
    rnd = random.Random(seed)
    parts: list[str] = []
    size = 0
    while size < n_bytes:
        r = rnd.random()
        if r < 0.06:
            s = "## " + " ".join(w.capitalize() for w in rnd.choices(WORDS, k=rnd.randint(2, 4))) + "\n\n"
        elif r < 0.12:
            s = "".join(
                f"- {' '.join(rnd.choices(WORDS, k=rnd.randint(4, 9)))}.\n"
                for _ in range(rnd.randint(3, 6))
            ) + "\n"
        else:
            sents = []
            for _ in range(rnd.randint(3, 6)):
                words = " ".join(rnd.choices(WORDS, k=rnd.randint(6, 22)))
                sents.append(words.capitalize() + rnd.choice([". ", ". ", ". ", "! ", "? "]))
            s = " ".join(sents).strip() + "\n\n"
        parts.append(s)
        size += len(s)
    return _trim("".join(parts), n_bytes)


def make_markdown(n_bytes: int, seed: int = 7) -> str:
    rnd = random.Random(seed)
    parts: list[str] = []
    size = 0
    level = 1
    while size < n_bytes:
        r = rnd.random()
        if r < 0.10:
            level = rnd.randint(1, 3)
            s = "#" * level + " " + " ".join(w.capitalize() for w in rnd.choices(WORDS, k=rnd.randint(2, 4))) + "\n\n"
        elif r < 0.18:
            s = (
                "| metric | value | delta |\n| --- | --- | --- |\n"
                + "".join(
                    f"| {' '.join(rnd.choices(WORDS, k=2))} | {rnd.random():.4f} | {rnd.randint(-9, 9)}% |\n"
                    for _ in range(rnd.randint(3, 7))
                )
                + "\n"
            )
        elif r < 0.26:
            s = "```python\n" + "".join(
                f"{' ' * rnd.randint(0, 8)}{rnd.choice(['if', 'for', 'return', 'yield'])} {' '.join(rnd.choices(WORDS, k=3))}\n"
                for _ in range(rnd.randint(3, 8))
            ) + "```\n\n"
        else:
            s = " ".join(
                " ".join(rnd.choices(WORDS, k=rnd.randint(10, 30))).capitalize() + "."
                for _ in range(rnd.randint(2, 5))
            ) + "\n\n"
        parts.append(s)
        size += len(s)
    return _trim("".join(parts), n_bytes)


def make_python(n_bytes: int, seed: int = 11) -> str:
    rnd = random.Random(seed)
    blocks: list[str] = []
    size = 0
    fn = 0
    while size < n_bytes:
        fn += 1
        body = "".join(
            f"    {rnd.choice(['total += ', 'acc *= ', 'cache[' + str(i) + '] = ', 'offset = '])}"
            f"{rnd.choice(['value', 'len(items)', 'x * 2', 'state.get(key, 0)'])}"
            f"  # {rnd.choice(['fast path', 'guard', 'hot loop', 'fallback'])}\n"
            for i in range(rnd.randint(4, 12))
        )
        block = (
            f"def transform_{fn}(items, key=None, *, limit={rnd.randint(8, 512)}):\n"
            f'    """{rnd.choice(["Normalize", "Aggregate", "Encode", "Rank"])} {rnd.choice(WORDS)} batch."""\n'
            f"    total = 0\n    acc = 1\n    cache = {{}}\n    state = {{}}\n    offset = 0\n"
            f"{body}"
            f"    if limit is not None and total > limit:\n        return cache\n"
            f"    return {{'total': total, 'acc': acc, 'offset': offset}}\n\n\n"
        )
        blocks.append(block)
        size += len(block)
    return _trim("".join(blocks), n_bytes)


# --------------------------------------------------------------------------------------
# plumbing
# --------------------------------------------------------------------------------------


@dataclass
class Impl:
    name: str
    lib: str
    fn: Callable[[Any], Any]
    build_ms: float = 0.0
    max_bytes: int | None = None
    note: str = ""


@dataclass
class Case:
    id: str
    corpus: str
    text: Any
    n_bytes: int
    params: str
    impls: list[Impl] = field(default_factory=list)
    note: str = ""


def call(method: str, obj: Any) -> Callable[[Any], Any]:
    """Bind an unbound API call: `call("chunk", SomeChunker(...))` -> `obj.chunk`."""
    return getattr(obj, method)


def timed_ms(fn: Callable[[Any], Any], arg: Any) -> float:
    t0 = time.perf_counter_ns()
    fn(arg)
    return (time.perf_counter_ns() - t0) / 1e6


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


def try_impl(name: str, lib: str, factory: Callable[[], Callable[[Any], Any]], **kw: Any) -> Impl | None:
    t0 = time.perf_counter()
    try:
        fn = factory()
    except Exception as exc:  # noqa: BLE001 - benchmark must survive missing/broken deps
        print(f"    ! {name} unavailable: {type(exc).__name__}: {exc}", file=sys.stderr)
        return None
    build_ms = (time.perf_counter() - t0) * 1000
    return Impl(name=name, lib=lib, fn=fn, build_ms=build_ms, **kw)


def run_case(case: Case, warmup: int, reps: int) -> list[dict[str, Any]]:
    impls = [i for i in case.impls if i.max_bytes is None or case.n_bytes <= i.max_bytes]
    if not impls:
        return []

    for impl in impls:  # warmup: warms caches, regex/tokenizer tables
        for _ in range(warmup):
            impl.fn(case.text)

    # Pick inner iterations so each timed sample spans >= ~5 ms (timer-noise control).
    inner: dict[str, int] = {}
    for impl in impls:
        per_call_ms = max(min(timed_ms(impl.fn, case.text) for _ in range(3)), 1e-3)
        inner[impl.name] = max(1, min(200, int(5.0 / per_call_ms)))

    times: dict[str, list[float]] = {i.name: [] for i in impls}
    counts: dict[str, list[int]] = {i.name: [] for i in impls}
    for _ in range(reps):
        for impl in impls:  # round-robin: one timed sample per impl per round
            n = inner[impl.name]
            t0 = time.perf_counter_ns()
            for _ in range(n):
                res = impl.fn(case.text)
            times[impl.name].append((time.perf_counter_ns() - t0) / 1e6 / n)
            counts[impl.name].append(len(res) if isinstance(res, (list, tuple)) else 1)

    rows: list[dict[str, Any]] = []
    for impl in impls:
        ts = times[impl.name]
        med = statistics.median(ts)
        chunks = contents(impl.fn(case.text))
        total_chars = sum(len(c) for c in chunks)
        rows.append(
            {
                "case": case.id,
                "corpus": case.corpus,
                "n_bytes": case.n_bytes,
                "impl": impl.name,
                "lib": impl.lib,
                "median_ms": round(med, 4),
                "min_ms": round(min(ts), 4),
                "p90_ms": round(statistics.quantiles(ts, n=10)[8] if len(ts) > 3 else max(ts), 4),
                "mbps": round(case.n_bytes / (med / 1000) / MB, 2),
                "reps_ms": [round(t, 4) for t in ts],
                "inner_calls": inner[impl.name],
                "chunks": len(chunks),
                "avg_chunk_chars": round(total_chars / len(chunks), 1) if chunks else 0,
                "chars_coverage": round(total_chars / case.n_bytes, 3),
                "chunk_count_stable": len(set(counts[impl.name])) == 1,
                "spread_pct": round((max(ts) - min(ts)) / med * 100, 1) if med else 0.0,
                "noisy": bool(med and (max(ts) - min(ts)) / med > 0.25),
                "build_ms": round(impl.build_ms, 3),
                "note": impl.note,
            }
        )

    best_alt = min(
        (r["median_ms"] for r in rows if r["lib"] != "chunkr"),
        default=None,
    )
    for r in rows:
        r["speedup_vs_best_alt"] = (
            round(best_alt / r["median_ms"], 2) if best_alt and r["lib"] != "chunkr" else None
        )
    return sorted(rows, key=lambda r: r["median_ms"])


def cold_start(name: str, code: str) -> dict[str, Any]:
    """Encoder/tokenizer load cost on first use, measured in a fresh interpreter."""
    proc = subprocess.run([sys.executable, "-c", code], capture_output=True, text=True)
    if proc.returncode != 0:
        return {"impl": name, "error": proc.stderr.strip().splitlines()[-1] if proc.stderr else "failed"}
    return {"impl": name, "first_use_ms": round(float(proc.stdout.strip()) * 1000, 2)}


def env_meta() -> dict[str, Any]:
    def sh(*args: str) -> str:
        try:
            return subprocess.run(args, capture_output=True, text=True, timeout=15).stdout.strip()
        except Exception:  # noqa: BLE001
            return ""

    import importlib.metadata as md

    pkgs = [
        "chunkr-rs",
        "langchain-text-splitters",
        "llama-index-core",
        "chonkie",
        "chonkie-core",
        "semchunk",
        "semantic-text-splitter",
        "tiktoken",
    ]
    versions = {}
    for p in pkgs:
        try:
            versions[p] = md.version(p)
        except Exception:  # noqa: BLE001
            versions[p] = None

    return {
        "timestamp": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "machine": sh("sysctl", "-n", "machdep.cpu.brand_string") or platform.processor(),
        "cpu_count": sh("sysctl", "-n", "hw.ncpu") or str(os_cpu_count()),
        "physical_cores": sh("sysctl", "-n", "hw.physicalcpu"),
        "ram_gb": round(int(sh("sysctl", "-n", "hw.memsize") or 0) / 1e9, 1) or None,
        "os": f"{platform.system()} {platform.release()} ({sh('sw_vers', '-productVersion')})".strip(),
        "python": platform.python_version(),
        "git_sha": sh("git", "-C", str(ROOT), "rev-parse", "--short", "HEAD"),
        "git_dirty": bool(sh("git", "-C", str(ROOT), "status", "--porcelain")),
        "packages": versions,
        "units": "MB = 10^6 bytes; MB/s = file bytes / wall time",
    }


def os_cpu_count() -> int:
    import os

    return os.cpu_count() or 0


# --------------------------------------------------------------------------------------
# benchmark cases
# --------------------------------------------------------------------------------------


def build_cases() -> list[Case]:
    import chunkr
    import chonkie
    import semchunk
    import semantic_text_splitter
    import tree_sitter_python
    import llama_index.core as llama_doc
    import llama_index.core.node_parser as llama_parser

    prose = {s: make_prose(n) for s, n in (("100kb", 100_000), ("1mb", 1 * MB), ("5mb", 5 * MB))}
    md = make_markdown(500_000)
    code = make_python(200_000)
    tok = make_prose(200_000, seed=99)
    sent = make_prose(500_000, seed=5)
    batch = [make_prose(50_000, seed=100 + i) for i in range(10)] * 10  # 100 docs x 50 KB

    cases: list[Case] = []
    slow = 2 * MB  # pure-Python splitters excluded above this size to bound runtime

    # --- 1. recursive character splitting (chunkr's headline claim) -------------------
    for label, text in (("1mb", prose["1mb"]), ("5mb", prose["5mb"])):
        from langchain_text_splitters import RecursiveCharacterTextSplitter

        c = Case(
            id=f"recursive_{label}",
            corpus="prose",
            text=text,
            n_bytes=len(text.encode()),
            params="chunk_size=1000 chars, overlap=200",
        )
        c.impls = [
            i
            for i in [
                try_impl("chunkr RecursiveChunker", "chunkr", lambda: call("chunk", chunkr.RecursiveChunker(1000, 200))),
                try_impl("langchain RecursiveCharacterTextSplitter", "langchain", lambda: call("split_text", RecursiveCharacterTextSplitter(chunk_size=1000, chunk_overlap=200))),
                try_impl("chonkie RecursiveChunker (rust core)", "chonkie", lambda: call("chunk", chonkie.RecursiveChunker(tokenizer="character", chunk_size=1000)), note="no overlap parameter"),
                try_impl("semchunk chunk(token_counter=len)", "semchunk", lambda: (lambda s: semchunk.chunk(s, chunk_size=1000, token_counter=len, overlap=200))),
                try_impl("text-splitter TextSplitter (rust)", "text-splitter", lambda: call("chunks", semantic_text_splitter.TextSplitter(1000, 200))),
                try_impl("llama-index SentenceSplitter", "llamaindex", lambda: (lambda s: llama_parser.SentenceSplitter(chunk_size=1000, chunk_overlap=200).get_nodes_from_documents([llama_doc.Document(text=s)])), max_bytes=slow, note="closest llama-index analogue (token-budgeted)"),
            ]
            if i
        ]
        cases.append(c)

    # --- 2. fixed-width character splitting ------------------------------------------
    text = prose["1mb"]
    c = Case(
        id="fixed_char_1mb",
        corpus="prose",
        text=text,
        n_bytes=len(text.encode()),
        params="chunk_size=1000 chars, overlap=200",
    )
    from langchain_text_splitters import CharacterTextSplitter

    c.impls = [
        i
        for i in [
            try_impl("chunkr CharacterChunker", "chunkr", lambda: call("chunk", chunkr.CharacterChunker(1000, 200))),
            try_impl("langchain CharacterTextSplitter(sep='')", "langchain", lambda: call("split_text", CharacterTextSplitter(separator="", chunk_size=1000, chunk_overlap=200)), note="langchain's fixed-width splitter"),
            try_impl("langchain RecursiveCharacterTextSplitter(separators=[''])", "langchain", lambda: call("split_text", RecursiveCharacterTextSplitter(separators=[""], chunk_size=1000, chunk_overlap=200)), note="common workaround for fixed-width splitting"),
            try_impl("chonkie TokenChunker(character)", "chonkie", lambda: call("chunk", chonkie.TokenChunker(tokenizer="character", chunk_size=1000, chunk_overlap=200))),
        ]
        if i
    ]
    cases.append(c)

    # --- 3. markdown ------------------------------------------------------------------
    c = Case(
        id="markdown_500kb",
        corpus="markdown",
        text=md,
        n_bytes=len(md.encode()),
        params="chunk_size=1000 chars, overlap=150",
    )
    from langchain_text_splitters import MarkdownHeaderTextSplitter

    c.impls = [
        i
        for i in [
            try_impl("chunkr MarkdownChunker", "chunkr", lambda: call("chunk", chunkr.MarkdownChunker(1000, 150))),
            try_impl("langchain MarkdownHeaderTextSplitter", "langchain", lambda: call("split_text", MarkdownHeaderTextSplitter(headers_to_split_on=[("#", "h1"), ("##", "h2"), ("###", "h3")])), note="header-only split, no size budget"),
            try_impl("text-splitter MarkdownSplitter (rust)", "text-splitter", lambda: call("chunks", semantic_text_splitter.MarkdownSplitter(1000, 150))),
            try_impl("llama-index MarkdownNodeParser", "llamaindex", lambda: (lambda s: llama_parser.MarkdownNodeParser().get_nodes_from_documents([llama_doc.Document(text=s)])), note="header-only split, no size budget"),
        ]
        if i
    ]
    cases.append(c)

    # --- 4. python source -------------------------------------------------------------
    c = Case(
        id="code_python_200kb",
        corpus="python",
        text=code,
        n_bytes=len(code.encode()),
        params="chunk_size=1500 chars, overlap=200",
    )
    from langchain_text_splitters import Language, RecursiveCharacterTextSplitter

    c.impls = [
        i
        for i in [
            try_impl("chunkr CodeChunker(python)", "chunkr", lambda: call("chunk", chunkr.CodeChunker("python", 1500, 200))),
            try_impl("chunkr AstCodeChunker(python, tree-sitter)", "chunkr", lambda: call("chunk", chunkr.AstCodeChunker("python", 1500)), note="AST boundaries, no overlap"),
            try_impl("langchain Recursive(PYTHON)", "langchain", lambda: call("split_text", RecursiveCharacterTextSplitter.from_language(Language.PYTHON, chunk_size=1500, chunk_overlap=200)), note="regex separators"),
            try_impl("text-splitter CodeSplitter (rust, tree-sitter)", "text-splitter", lambda: call("chunks", semantic_text_splitter.CodeSplitter(tree_sitter_python.language(), 1500, 200))),
        ]
        if i
    ]
    cases.append(c)

    # --- 5. BPE token chunking (cl100k_base) ------------------------------------------
    c = Case(
        id="token_cl100k_200kb",
        corpus="prose",
        text=tok,
        n_bytes=len(tok.encode()),
        params="chunk_size=512 tokens, overlap=50",
    )
    from langchain_text_splitters import TokenTextSplitter

    c.impls = [
        i
        for i in [
            try_impl("chunkr TokenChunker(cl100k_base)", "chunkr", lambda: call("chunk", chunkr.TokenChunker(512, 50, "cl100k_base"))),
            try_impl("langchain TokenTextSplitter(cl100k_base)", "langchain", lambda: call("split_text", TokenTextSplitter(encoding_name="cl100k_base", chunk_size=512, chunk_overlap=50))),
            try_impl("chonkie TokenChunker(cl100k_base)", "chonkie", lambda: call("chunk", chonkie.TokenChunker(tokenizer="cl100k_base", chunk_size=512, chunk_overlap=50))),
            try_impl("text-splitter TextSplitter.from_tiktoken_model", "text-splitter", lambda: call("chunks", semantic_text_splitter.TextSplitter.from_tiktoken_model("gpt-4", 512, 50))),
            try_impl("llama-index TokenTextSplitter(cl100k_base)", "llamaindex", lambda: (lambda s: llama_parser.TokenTextSplitter(chunk_size=512, chunk_overlap=50).get_nodes_from_documents([llama_doc.Document(text=s)]))),
        ]
        if i
    ]
    cases.append(c)

    # --- 6. sentence splitting --------------------------------------------------------
    c = Case(
        id="sentence_500kb",
        corpus="prose",
        text=sent,
        n_bytes=len(sent.encode()),
        params="chunkr: 3 sentences/chunk, overlap=1 (sentence-count semantics)",
        note="chunkr's SentenceChunker is sentence-count based; alternatives are size-budgeted - compare pass speed, not identical output",
    )
    c.impls = [
        i
        for i in [
            try_impl("chunkr SentenceChunker(3 sentences)", "chunkr", lambda: call("chunk", chunkr.SentenceChunker(3, 1))),
            try_impl("chonkie SentenceChunker(character, 1000)", "chonkie", lambda: call("chunk", chonkie.SentenceChunker(tokenizer="character", chunk_size=1000, chunk_overlap=0))),
            try_impl("llama-index SentenceSplitter(1000)", "llamaindex", lambda: (lambda s: llama_parser.SentenceSplitter(chunk_size=1000, chunk_overlap=0).get_nodes_from_documents([llama_doc.Document(text=s)]))),
        ]
        if i
    ]
    cases.append(c)

    # --- 7. multi-document batch (chunkr's Rayon/GIL-release path) --------------------
    c = Case(
        id="batch_100x50kb",
        corpus="prose",
        text=batch,
        n_bytes=sum(len(t.encode()) for t in batch),
        params="100 docs x 50 KB, chunk_size=1000, overlap=200",
        note="chunkr par_chunk_texts uses Rayon (GIL released); langchain has no parallel API - loop is its best available",
    )
    c.impls = [
        i
        for i in [
            try_impl("chunkr RecursiveChunker.par_chunk_texts", "chunkr", lambda: call("par_chunk_texts", chunkr.RecursiveChunker(1000, 200))),
            try_impl("chunkr RecursiveChunker.chunk (loop)", "chunkr", lambda: (lambda ch: lambda ts: [ch.chunk(t) for t in ts])(chunkr.RecursiveChunker(1000, 200))),
            try_impl("langchain RecursiveCharacterTextSplitter (loop)", "langchain", lambda: (lambda ch: lambda ts: [ch.split_text(t) for t in ts])(RecursiveCharacterTextSplitter(chunk_size=1000, chunk_overlap=200))),
            try_impl("chonkie RecursiveChunker (loop)", "chonkie", lambda: (lambda ch: lambda ts: [ch.chunk(t) for t in ts])(chonkie.RecursiveChunker(tokenizer="character", chunk_size=1000))),
        ]
        if i
    ]
    cases.append(c)
    return cases


COLD_START_SCRIPTS = {
    "chunkr TokenChunker(cl100k) build + first chunk": (
        "import time; import chunkr; t=time.perf_counter(); ch=chunkr.TokenChunker(512,50,'cl100k_base');"
        "ch.chunk('hello world '*200); print(time.perf_counter()-t)"
    ),
    "langchain TokenTextSplitter(cl100k) build + first chunk": (
        "import time; from langchain_text_splitters import TokenTextSplitter; t=time.perf_counter();"
        "ch=TokenTextSplitter(encoding_name='cl100k_base', chunk_size=512, chunk_overlap=50);"
        "ch.split_text('hello world '*200); print(time.perf_counter()-t)"
    ),
    "chonkie TokenChunker(cl100k) build + first chunk": (
        "import time; from chonkie import TokenChunker; t=time.perf_counter();"
        "ch=TokenChunker(tokenizer='cl100k_base', chunk_size=512, chunk_overlap=50);"
        "ch.chunk('hello world '*200); print(time.perf_counter()-t)"
    ),
    "text-splitter from_tiktoken_model build + first chunk": (
        "import time; from semantic_text_splitter import TextSplitter; t=time.perf_counter();"
        "ch=TextSplitter.from_tiktoken_model('gpt-4', 512, 50);"
        "ch.chunks('hello world '*200); print(time.perf_counter()-t)"
    ),
}


# --------------------------------------------------------------------------------------
# reporting
# --------------------------------------------------------------------------------------


def render_markdown(rows: list[dict[str, Any]], meta: dict[str, Any], cold: list[dict[str, Any]]) -> str:
    lines = [
        "# chunkr benchmark results",
        "",
        f"- run: `{meta['timestamp']}`",
        f"- machine: {meta['machine']} | {meta['cpu_count']} logical / {meta['physical_cores']} physical cores | {meta['ram_gb']} GB RAM",
        f"- os: {meta['os']} | python {meta['python']} | git {meta['git_sha']}{' (dirty)' if meta['git_dirty'] else ''}",
        "- units: MB = 10^6 bytes; median of reps; chunker construction excluded from timing",
        "",
        "| package | version |",
        "| --- | --- |",
    ]
    lines += [f"| {k} | {v} |" for k, v in meta["packages"].items() if v]
    lines.append("")
    if cold:
        lines += [
            "## Cold start (encoder build + first chunk, fresh interpreter)",
            "",
            "| implementation | first_use_ms |",
            "| --- | --- |",
        ]
        lines += [f"| {c['impl']} | {c.get('first_use_ms', c.get('error'))} |" for c in cold]
        lines.append("")

    for case_id in dict.fromkeys(r["case"] for r in rows):
        case_rows = [r for r in rows if r["case"] == case_id]
        head = next(r for r in case_rows if r["lib"] == "chunkr")
        lines += [
            f"## {case_id}",
            "",
            f"corpus `{head['corpus'] if 'corpus' in head else head['case']}` | {head['n_bytes'] / MB:.3f} MB | median of {len(head['reps_ms'])} reps | construction excluded",
            "",
            "| implementation | library | median ms | MB/s | chunks | avg chunk chars | vs fastest alternative |",
            "| --- | --- | --- | --- | --- | --- | --- |",
        ]
        for r in case_rows:
            sp = f"{r['speedup_vs_best_alt']}x" if r["speedup_vs_best_alt"] else "baseline"
            lines.append(
                f"| {r['impl']} | {r['lib']} | {r['median_ms']} | {r['mbps']} | {r['chunks']} | {r['avg_chunk_chars']} | {sp} |"
            )
        notes = {r["note"] for r in case_rows if r["note"]}
        if notes:
            lines += ["", *(f"> {n}" for n in sorted(notes))]
        lines.append("")
    return "\n".join(lines)


def print_case(case: Case, rows: list[dict[str, Any]]) -> None:
    print(f"\n== {case.id} | {case.corpus} | {case.n_bytes / MB:.3f} MB | {case.params}")
    for r in rows:
        sp = f"  ({r['speedup_vs_best_alt']}x vs best alt)" if r["speedup_vs_best_alt"] else ""
        print(
            f"   {r['impl']:<58} {r['median_ms']:>9.3f} ms  {r['mbps']:>9.1f} MB/s  "
            f"{r['chunks']:>6} chunks  {r['avg_chunk_chars']:>7.1f} avg chars  "
            f"[{r['inner_calls']}x, spread {r['spread_pct']}%{'!' if r['noisy'] else ''}]{sp}"
        )


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--reps", type=int, default=7)
    ap.add_argument("--warmup", type=int, default=2)
    ap.add_argument("--only", default=None, help="run only cases whose id contains this substring")
    ap.add_argument("--quick", action="store_true", help="3 reps, skip the 5 MB case")
    ap.add_argument("--skip-cold-start", action="store_true")
    ap.add_argument("--outdir", type=Path, default=HERE / "results")
    args = ap.parse_args()

    reps = 3 if args.quick else args.reps
    cases = build_cases()
    if args.quick:
        cases = [c for c in cases if c.n_bytes <= 2 * MB]
    if args.only:
        cases = [c for c in cases if args.only in c.id]
    if not cases:
        print("no cases matched", file=sys.stderr)
        return 2

    meta = env_meta()
    corpus_hashes = {
        c.id: hashlib.sha256(
            (c.text.encode() if isinstance(c.text, str) else b"".join(t.encode() for t in c.text))
        ).hexdigest()[:16]
        for c in cases
    }
    print(f"machine: {meta['machine']} ({meta['cpu_count']} threads) | python {meta['python']} | git {meta['git_sha']}")
    print(f"reps={reps} warmup={args.warmup} | corpora sha256: {corpus_hashes}")

    all_rows: list[dict[str, Any]] = []
    gc.disable()
    try:
        for case in cases:
            rows = run_case(case, args.warmup, reps)
            if rows:
                print_case(case, rows)
                all_rows.extend(rows)
    finally:
        gc.enable()

    cold = []
    if not args.skip_cold_start:
        print("\n== cold start (fresh interpreter: encoder build + first chunk)")
        for name, code in COLD_START_SCRIPTS.items():
            result = cold_start(name, code)
            cold.append(result)
            print(f"   {name:<58} {result.get('first_use_ms', result.get('error'))} ms")

    args.outdir.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    payload = {
        "meta": meta,
        "config": {"reps": reps, "warmup": args.warmup, "corpora_sha256": corpus_hashes},
        "cold_start": cold,
        "results": all_rows,
    }
    json_path = args.outdir / f"chunking-{stamp}.json"
    json_path.write_text(json.dumps(payload, indent=2))
    md_path = args.outdir / f"chunking-{stamp}.md"
    md_path.write_text(render_markdown(all_rows, meta, cold))
    print(f"\nwrote {json_path.relative_to(ROOT)} and {md_path.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
