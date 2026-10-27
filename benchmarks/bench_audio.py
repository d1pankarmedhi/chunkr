#!/usr/bin/env python3
"""Audio and speech chunking benchmark.

The text harness asks whether a retrieved chunk contains the answer. Audio needs
three different questions, because everything downstream of it is time:

1. **Do chunk boundaries land in silence?** Speech cut mid-word is unrecoverable;
   a seam in a pause costs nothing. Gold is exact here: the corpus is real speech
   glued together with known silent gaps, so every seam either falls inside a
   clip (bad) or inside a gap (good).
2. **Does the speech accounting match the audio?** `speech_s`/`speech_ratio` are
   what callers see when they decide whether a file has speech at all; they are
   compared against the duration of speech actually inserted.
3. **Do caption/transcript windows serve retrieval?** Cue times are known, so a
   query drawn from one cue gives a temporal recall (was the cue inside a
   returned window?) and a temporal precision (how much of the window is the
   cue?) — the audio analogue of the text harness's `hit@k` and `P@k`.

With a transcriber available (``--transcriber``) it also measures word error rate
end to end, windows off vs on, which is the only way to see whether pre-chunking
damages transcription.

    python benchmarks/bench_audio.py                     # no model needed, ~1 min
    python benchmarks/bench_audio.py --clips 40 --seam-seconds 20 30
    python benchmarks/bench_audio.py --transcriber whisper --model tiny

Corpus: `google/fleurs` en_us dev (CC BY 4.0), streamed from the pinned revision
and cut to the first ``--clips`` utterances, so the download stays small; the
clips, the joined WAV, the caption track and the manifest are cached under
``benchmarks/data/audio_hf``.
"""

from __future__ import annotations

import argparse
import io
import json
import math
import os
import random
import re
import struct
import sys
import tarfile
import time
import urllib.error
import urllib.request
import wave
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Sequence

DATA_DIR = Path(__file__).resolve().parent / "data" / "audio_hf"
RESULTS_DIR = Path(__file__).resolve().parent / "results"

FLEURS_REPO = "google/fleurs"
FLEURS_URL = (
    "https://huggingface.co/datasets/google/fleurs/resolve/{revision}"
    "/data/en_us/audio/dev.tar.gz"
)
FLEURS_REVISION = "70bb2e84b976b7e960aa89f1c648e09c59f894dd"
FLEURS_TSV = (
    "https://huggingface.co/datasets/google/fleurs/resolve/{revision}/data/en_us/dev.tsv"
)

# Silence inserted between clips. Wide enough to be a seam target, narrow enough
# that the corpus stays realistic speech-dense audio.
GAP_S = 0.5
RATE = 16_000


# ------------------------------------------------------------------------ corpus


def http_open(url: str, retries: int = 3, timeout: int = 120):
    last: Exception | None = None
    for attempt in range(retries):
        try:
            request = urllib.request.Request(url, headers={"User-Agent": "chunkr-bench/1.0"})
            return urllib.request.urlopen(request, timeout=timeout)
        except (urllib.error.URLError, TimeoutError) as error:  # transient
            last = error
            time.sleep(1.5 * (attempt + 1))
    raise SystemExit(f"download failed: {url}: {last}")


def read_transcripts(revision: str, cache: Path) -> dict[str, str]:
    """Gold text per FLEURS clip, from the dev manifest (2 MB at most)."""
    target = cache / "dev.tsv"
    if not target.exists():
        with http_open(FLEURS_TSV.format(revision=revision)) as response:
            target.write_bytes(response.read())
    gold: dict[str, str] = {}
    for line in target.read_text(encoding="utf-8").splitlines():
        fields = line.split("\t")
        if len(fields) >= 4:
            gold[fields[1].strip()] = fields[2].strip()
    return gold


def fetch_clips(revision: str, count: int, cache: Path) -> list[tuple[str, bytes]]:
    """Stream the FLEURS dev tarball and keep the first `count` clips.

    The archive is 171 MB but clips arrive in order, so the connection is closed
    as soon as enough of them are in hand — a benchmark should not spend minutes
    downloading speech it will never transcribe.
    """
    url = FLEURS_URL.format(revision=revision)
    clips: list[tuple[str, bytes]] = []
    with http_open(url, timeout=180) as response:
        with tarfile.open(fileobj=response, mode="r|gz") as archive:
            for member in archive:
                if not member.isfile() or not member.name.endswith(".wav"):
                    continue
                handle = archive.extractfile(member)
                if handle is None:
                    continue
                clips.append((Path(member.name).name, handle.read()))
                if len(clips) >= count:
                    break
    return clips


def wav_samples(payload: bytes) -> tuple[list[int], int]:
    """Mono 16-bit samples and rate, from PCM or float WAV.

    FLEURS ships IEEE-float WAVs and `chunkr` writes 16-bit PCM, so this reads both
    rather than assuming the corpus looks like our own output.
    """
    if payload[:4] not in (b"RIFF", b"RF64"):
        raise SystemExit("bench_audio: not a WAV payload")
    tag = channels = rate = bits = 0
    data = b""
    offset = 12
    while offset + 8 <= len(payload):
        chunk_id = payload[offset : offset + 4]
        (size,) = struct.unpack("<I", payload[offset + 4 : offset + 8])
        body = offset + 8
        if chunk_id == b"fmt ":
            tag, channels = struct.unpack("<HH", payload[body : body + 4])
            (rate,) = struct.unpack("<I", payload[body + 4 : body + 8])
            bits = struct.unpack("<H", payload[body + 14 : body + 16])[0]
            if tag == 0xFFFE and size >= 40:  # WAVE_FORMAT_EXTENSIBLE
                (tag,) = struct.unpack("<H", payload[body + 24 : body + 26])
        elif chunk_id == b"data":
            data = payload[body : body + size]
            break
        offset = body + size + (size & 1)

    if tag == 3 and bits == 32:
        floats = struct.unpack(f"<{len(data) // 4}f", data)
        samples = [int(max(-1.0, min(1.0, value)) * 32767) for value in floats]
    elif tag == 1 and bits == 16:
        samples = list(struct.unpack(f"<{len(data) // 2}h", data))
    else:
        raise SystemExit(f"bench_audio: unsupported WAV (format {tag}, {bits} bits)")
    if channels > 1:  # average down to mono
        samples = [
            sum(samples[index : index + channels]) // channels
            for index in range(0, len(samples) - channels + 1, channels)
        ]
    return samples, rate


@dataclass
class Gold:
    """One utterance, placed at a known point on the timeline."""

    clip: str
    start_s: float
    end_s: float
    text: str
    gain_db: float = 0.0


@dataclass
class Corpus:
    wav: Path
    srt: Path
    manifest: Path
    gold: list[Gold] = field(default_factory=list)
    duration_s: float = 0.0
    gaps: list[tuple[float, float]] = field(default_factory=list)

    @property
    def speech_s(self) -> float:
        return sum(item.end_s - item.start_s for item in self.gold)


def srt_time(seconds: float) -> str:
    millis = int(round(seconds * 1000))
    hours, rest = divmod(millis, 3_600_000)
    minutes, rest = divmod(rest, 60_000)
    secs, millis = divmod(rest, 1000)
    return f"{hours:02d}:{minutes:02d}:{secs:02d},{millis:03d}"


def normalise(samples: list[int], target_peak: float) -> tuple[list[int], float]:
    """Peak-normalise one clip; returns the samples and the gain applied, in dB."""
    peak = max((abs(value) for value in samples), default=0)
    if peak == 0:
        return samples, 0.0
    gain = target_peak / (peak / 32768)
    if abs(gain - 1.0) < 0.01:
        return samples, 0.0
    return [max(-32768, min(32767, int(value * gain))) for value in samples], 20 * math.log10(gain)


def build_corpus(
    data_dir: Path,
    revision: str,
    clips_wanted: int,
    gap_s: float,
    target_peak: float = 0.5,
) -> Corpus:
    """Glue real speech into one file, with a caption cue per utterance.

    Clips are peak-normalised to a common level. FLEURS loudness spans 40 dB
    (peaks from 0.005 to 0.96), and a transcriber that silently drops quiet speech
    would make the windowing comparison measure gain, not chunking.
    """
    root = data_dir / (
        f"fleurs_{revision[:10]}_{clips_wanted}_{int(gap_s * 1000)}ms_peak{int(target_peak * 100)}"
    )
    corpus = Corpus(
        wav=root / "speech.wav",
        srt=root / "speech.srt",
        manifest=root / "manifest.json",
    )
    if corpus.manifest.exists():
        payload = json.loads(corpus.manifest.read_text())
        corpus.gold = [Gold(**item) for item in payload["gold"]]
        corpus.gaps = [tuple(gap) for gap in payload["gaps"]]
        corpus.duration_s = payload["duration_s"]
        return corpus

    root.mkdir(parents=True, exist_ok=True)
    transcripts = read_transcripts(revision, root)
    clips = fetch_clips(revision, clips_wanted, root)

    samples: list[int] = []
    gold: list[Gold] = []
    gaps: list[tuple[float, float]] = []
    cursor = 0
    for name, payload in clips:
        text = transcripts.get(name, "").strip()
        clip_samples, rate = wav_samples(payload)
        if rate != RATE or not text:
            continue
        clip_samples, gain_db = normalise(clip_samples, target_peak)
        if gold:  # a silent gap before every clip but the first
            gap = len(samples) / RATE
            samples.extend([0] * int(gap_s * RATE))
            gaps.append((gap, gap + gap_s))
        start = len(samples) / RATE
        samples.extend(clip_samples)
        end = len(samples) / RATE
        gold.append(
            Gold(
                clip=name,
                start_s=round(start, 4),
                end_s=round(end, 4),
                text=text,
                gain_db=round(gain_db, 2),
            )
        )
        cursor += 1

    with wave.open(str(corpus.wav), "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(RATE)
        handle.writeframes(struct.pack(f"<{len(samples)}h", *samples))

    cues = []
    for index, item in enumerate(gold, start=1):
        cues.append(
            f"{index}\n{srt_time(item.start_s)} --> {srt_time(item.end_s)}\n{item.text}\n"
        )
    corpus.srt.write_text("\n".join(cues), encoding="utf-8")

    corpus.gold = gold
    corpus.gaps = gaps
    corpus.duration_s = len(samples) / RATE
    corpus.manifest.write_text(
        json.dumps(
            {
                "revision": revision,
                "duration_s": corpus.duration_s,
                "gap_s": gap_s,
                "gold": [item.__dict__ for item in gold],
                "gaps": gaps,
            },
            indent=2,
        )
    )
    print(
        f"   corpus: {len(gold)} utterances, {corpus.duration_s:.1f}s audio, "
        f"{corpus.speech_s:.1f}s speech ({100 * corpus.speech_s / corpus.duration_s:.0f}%)",
        file=sys.stderr,
    )
    return corpus


# ------------------------------------------------------------------ seam probing


def wav_duration(payload: bytes) -> float:
    """Duration of a WAV payload, read from its header rather than assumed."""
    if payload[:4] != b"RIFF":
        raise SystemExit("bench_audio: transcriber did not receive a WAV")
    offset = 12
    rate = channels = bits = 0
    while offset + 8 <= len(payload):
        chunk_id = payload[offset : offset + 4]
        (size,) = struct.unpack("<I", payload[offset + 4 : offset + 8])
        body = offset + 8
        if chunk_id == b"fmt ":
            channels, rate = struct.unpack("<H", payload[body + 2 : body + 4])[0], struct.unpack(
                "<I", payload[body + 4 : body + 8]
            )[0]
            bits = struct.unpack("<H", payload[body + 14 : body + 16])[0]
        elif chunk_id == b"data":
            return size / (channels * bits / 8) / rate
        offset = body + size + (size & 1)
    raise SystemExit("bench_audio: WAV has no data chunk")


def probe_seams(corpus: Corpus, window_s: float) -> dict[str, Any]:
    """Ask what the loader actually hands the model, window by window.

    A stand-in transcriber records the payload of every call; cumulative durations
    are the seams. That is the only view a backend gets, so it is the honest one
    to measure.
    """
    import chunkr

    durations: list[float] = []

    def transcribe(audio: bytes, audio_format: str):
        durations.append(wav_duration(audio))
        return [{"start": 0.0, "end": 0.1, "text": "probe"}]

    previous = os.environ.get("CHUNKR_AUDIO_WINDOW_S")
    chunkr.register_transcriber("bench-probe", transcribe)
    os.environ["CHUNKR_AUDIO_WINDOW_S"] = str(window_s)
    try:
        documents = chunkr.load_documents(str(corpus.wav))
    finally:
        chunkr.unregister_transcriber("bench-probe")
        if previous is None:
            os.environ.pop("CHUNKR_AUDIO_WINDOW_S", None)
        else:
            os.environ["CHUNKR_AUDIO_WINDOW_S"] = previous

    seams: list[float] = []
    cursor = 0.0
    for duration in durations[:-1]:
        cursor += duration
        seams.append(round(cursor, 4))

    frames, frame_s = frame_energies(corpus)
    speech_db = clip_speech_db(corpus, frames, frame_s)
    in_gap = in_speech = loud = 0
    distances: list[float] = []
    margins_db: list[float] = []
    for seam in seams:
        for start, end in corpus.gaps:
            if start <= seam <= end:
                in_gap += 1
                distances.append(min(seam - start, end - seam))
                break
        else:
            for item in corpus.gold:
                if item.start_s < seam < item.end_s:
                    in_speech += 1
                    index = int(seam / frame_s)
                    energy = frames[min(index, len(frames) - 1)]
                    reference = speech_db.get(item.clip, -60.0)
                    margins_db.append(reference - energy)
                    if energy > reference - 6.0:
                        loud += 1
                    break
    return {
        "window_s": window_s,
        "windows": len(durations),
        "max_window_s": max(durations) if durations else 0.0,
        "covered_s": round(sum(durations), 3),
        "coefficient_of_variation": round(
            statistics_pstdev(durations) / (sum(durations) / len(durations)) if durations else 0.0,
            3,
        ),
        "seams": len(seams),
        "seams_in_gap": in_gap,
        "seams_in_speech": in_speech,
        "gap_hit_rate": in_gap / max(len(seams), 1),
        "seam_in_clip_rate": in_speech / max(len(seams), 1),
        # Inside a clip is not the same as "cut a word": a 300 ms pause between
        # sentences lives inside a clip. Loudness at the seam is the honest test of
        # whether the cut went through speech, so the seam frame is compared with
        # the speech frames of the clip it lands in.
        "loud_seam_rate": loud / max(len(seams), 1),
        "median_seam_db_below_speech": round(statistics_median(margins_db), 1),
        "median_gap_margin_s": round(statistics_median(distances), 3),
        "speech_s": round(documents[0].metadata.get("speech_s") or 0.0, 3) if documents else None,
        "speech_ratio": (
            round(documents[0].metadata.get("speech_ratio") or 0.0, 4) if documents else None
        ),
        "speech_detected": documents[0].metadata.get("speech_detected") if documents else None,
        "segments": len(documents),
    }


def frame_energies(corpus: Corpus) -> tuple[list[float], float]:
    """Frame energies in dBFS at the loader's 50 ms analysis frame."""
    samples, rate = wav_samples(corpus.wav.read_bytes())
    size = int(0.05 * rate)
    frames = []
    for start in range(0, len(samples) - size, size):
        window = samples[start : start + size]
        mean_square = sum((value / 32768) ** 2 for value in window) / size
        frames.append(10 * math.log10(max(mean_square, 1e-12)))
    return frames, size / rate


def clip_speech_db(corpus: Corpus, frames: list[float], frame_s: float) -> dict[str, float]:
    """The 80th percentile frame energy of each clip: what its speech sounds like."""
    reference: dict[str, float] = {}
    for item in corpus.gold:
        inside = frames[int(item.start_s / frame_s) : int(item.end_s / frame_s)]
        if inside:
            ordered = sorted(inside)
            reference[item.clip] = ordered[int(0.8 * (len(ordered) - 1))]
    return reference


def statistics_pstdev(values: Sequence[float]) -> float:
    if len(values) < 2:
        return 0.0
    mean = sum(values) / len(values)
    return math.sqrt(sum((value - mean) ** 2 for value in values) / len(values))


def statistics_median(values: Sequence[float]) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[middle]
    return (ordered[middle - 1] + ordered[middle]) / 2


# ------------------------------------------------------------ caption retrieval


@dataclass
class Window:
    text: str
    start_s: float
    end_s: float
    cue_start: int
    cue_end: int


def caption_windows(corpus: Corpus, max_seconds: float) -> list[Window]:
    """What `SubtitleChunker` does with the same cue track."""
    import chunkr

    cues = chunkr.load_documents(str(corpus.srt))
    windows = chunkr.SubtitleChunker(max_seconds=max_seconds).chunk_documents(cues)
    out: list[Window] = []
    for window in windows:
        metadata = window.metadata
        out.append(
            Window(
                text=window.content,
                start_s=float(metadata["start_time_s"]),
                end_s=float(metadata["end_time_s"]),
                cue_start=int(metadata.get("cue_count", 0)),
                cue_end=0,
            )
        )
    return out


def fixed_windows(corpus: Corpus, max_seconds: float) -> list[Window]:
    """The naive baseline: cut the timeline every N seconds, cues be damned."""
    out: list[Window] = []
    for start in [index * max_seconds for index in range(int(corpus.duration_s / max_seconds) + 1)]:
        end = min(start + max_seconds, corpus.duration_s)
        if end - start < 0.001:
            continue
        pieces = [
            item.text
            for item in corpus.gold
            if item.end_s > start and item.start_s < end
        ]
        out.append(Window(" ".join(pieces), start, end, 0, 0))
    return out


def whole_windows(corpus: Corpus) -> list[Window]:
    return [
        Window(
            " ".join(item.text for item in corpus.gold),
            corpus.gold[0].start_s,
            corpus.gold[-1].end_s,
            0,
            0,
        )
    ]


TOKEN = re.compile(r"[a-z0-9']+")


def tokenize(text: str) -> list[str]:
    return TOKEN.findall(text.lower())


def bm25_rank(documents: list[str], query: str, k: int) -> list[int]:
    """Okapi BM25 over caption windows (k1=1.2, b=0.75), as in the text harness."""
    corpus_tokens = [tokenize(text) for text in documents]
    average = sum(len(tokens) for tokens in corpus_tokens) / max(len(corpus_tokens), 1)
    frequencies: dict[str, dict[int, int]] = {}
    for index, tokens in enumerate(corpus_tokens):
        for token in tokens:
            frequencies.setdefault(token, {})
            frequencies[token][index] = frequencies[token].get(index, 0) + 1
    scores = [0.0] * len(documents)
    terms = tokenize(query)
    for term in terms:
        postings = frequencies.get(term)
        if not postings:
            continue
        idf = math.log(1 + (len(documents) - len(postings) + 0.5) / (len(postings) + 0.5))
        for index, count in postings.items():
            length = len(corpus_tokens[index])
            norm = 1 - 0.75 + 0.75 * length / max(average, 1e-9)
            scores[index] += idf * count * 2.2 / (count + 1.2 * norm)
    return sorted(range(len(documents)), key=lambda index: scores[index], reverse=True)[:k]


def evaluate_captions(
    corpus: Corpus, windows: list[Window], k: int, seed: int, query_tokens: int = 3
) -> dict[str, Any]:
    """Query each cue by a phrase from its own text; gold is its time span."""
    random.seed(seed)
    texts = [window.text for window in windows]
    recall = precision = integrity = 0
    questions = 0
    for cue in corpus.gold:
        tokens = tokenize(cue.text)
        if len(tokens) < query_tokens + 1:
            continue
        start = random.randrange(0, len(tokens) - query_tokens)
        query = " ".join(tokens[start : start + query_tokens])
        questions += 1

        boundaries = {round(window.start_s, 3) for window in windows} | {
            round(window.end_s, 3) for window in windows
        }
        cut = any(cue.start_s + 1e-3 < edge < cue.end_s - 1e-3 for edge in boundaries)
        if not cut:
            integrity += 1

        for index in bm25_rank(texts, query, k):
            window = windows[index]
            overlap = min(window.end_s, cue.end_s) - max(window.start_s, cue.start_s)
            if overlap > 0.99 * (cue.end_s - cue.start_s):
                recall += 1
                precision += (cue.end_s - cue.start_s) / max(window.end_s - window.start_s, 1e-9)
                break
    return {
        "questions": questions,
        "windows": len(windows),
        "mean_window_s": (
            sum(window.end_s - window.start_s for window in windows) / max(len(windows), 1)
        ),
        "temporal_recall@k": recall / max(questions, 1),
        "temporal_precision@k": precision / max(questions, 1),
        "integrity": integrity / max(questions, 1),
    }


# ----------------------------------------------------------------- transcription


def words(text: str) -> list[str]:
    return tokenize(text)


def edit_distance(reference: list[str], hypothesis: list[str]) -> int:
    previous = list(range(len(hypothesis) + 1))
    for i, ref in enumerate(reference, start=1):
        current = [i]
        for j, hyp in enumerate(hypothesis, start=1):
            current.append(
                min(
                    previous[j] + 1,
                    current[j - 1] + 1,
                    previous[j - 1] + (ref != hyp),
                )
            )
        previous = current
    return previous[-1]


def wer(reference: str, hypothesis: str) -> float:
    ref = words(reference)
    return edit_distance(ref, words(hypothesis)) / max(len(ref), 1)


def transcribe_corpus(corpus: Corpus, window_s: float | None, transcriber: str | None) -> dict[str, Any]:
    """Word error rate against the gold transcripts, with and without windowing."""
    import chunkr

    previous_window = os.environ.get("CHUNKR_AUDIO_WINDOW_S")
    previous_backend = os.environ.get("CHUNKR_TRANSCRIBER")
    if window_s is None:
        os.environ.pop("CHUNKR_AUDIO_WINDOW_S", None)
    else:
        os.environ["CHUNKR_AUDIO_WINDOW_S"] = str(window_s)
    if transcriber:
        os.environ["CHUNKR_TRANSCRIBER"] = transcriber
    else:
        os.environ.pop("CHUNKR_TRANSCRIBER", None)
    try:
        documents = chunkr.load_documents(str(corpus.wav))
    finally:
        for name, value in (
            ("CHUNKR_AUDIO_WINDOW_S", previous_window),
            ("CHUNKR_TRANSCRIBER", previous_backend),
        ):
            if value is None:
                os.environ.pop(name, None)
            else:
                os.environ[name] = value

    # Global WER: the whole transcript against the whole reference, in time order.
    # Per-utterance attribution would be measuring the bucket, not the transcript —
    # a segment that legitimately spans two clips is not an error.
    hypothesis = " ".join(document.content.strip() for document in documents)
    reference = " ".join(item.text for item in corpus.gold)
    global_wer = wer(reference, hypothesis)

    # Per-clip view, but by segment *centre* so a straddling segment is counted once
    # and a clip that lost its words shows up as a deletion rather than as its
    # neighbour's extra words.
    buckets: list[list[str]] = [[] for _ in corpus.gold]
    for document in documents:
        centre = (float(document.metadata["start_time_s"]) + float(document.metadata["end_time_s"])) / 2
        for index, item in enumerate(corpus.gold):
            if item.start_s <= centre < item.end_s:
                buckets[index].append(document.content.strip())
                break

    errors = [wer(item.text, " ".join(buckets[index])) for index, item in enumerate(corpus.gold)]
    mean_wer = sum(errors) / max(len(errors), 1)
    word_counts = [document.metadata.get("word_count") or 0 for document in documents]
    order_ok = all(
        documents[index].metadata["start_time_s"] <= documents[index + 1].metadata["start_time_s"]
        for index in range(len(documents) - 1)
    )
    inside = all(
        0.0 <= document.metadata["start_time_s"] <= document.metadata["end_time_s"]
        <= corpus.duration_s + 0.01
        for document in documents
    )
    return {
        "window_s": window_s,
        "segments": len(documents),
        "utterances": len(errors),
        "wer": global_wer,
        "wer_per_utterance": mean_wer,
        "wer_worst": max(errors, default=0.0),
        "utterances_with_errors": sum(1 for value in errors if value > 0.2),
        "utterances_missing": sum(
            1 for index, item in enumerate(corpus.gold) if item.text and not buckets[index]
        ),
        "words": sum(word_counts),
        "segments_with_words": sum(1 for count in word_counts if count),
        "timing_monotonic": order_ok,
        "timing_inside_media": inside,
        "first_document": documents[0].content.strip()[:70] if documents else "",
    }


# ------------------------------------------------------------------------ report


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--clips", type=int, default=40, help="FLEURS utterances to glue together")
    parser.add_argument("--gap-ms", type=int, default=int(GAP_S * 1000), help="silence between clips")
    parser.add_argument(
        "--seam-seconds",
        nargs="+",
        type=float,
        default=[20.0, 30.0],
        help="CHUNKR_AUDIO_WINDOW_S values to probe",
    )
    parser.add_argument(
        "--caption-seconds",
        nargs="+",
        type=float,
        default=[15.0, 30.0, 60.0],
        help="SubtitleChunker max_seconds to score",
    )
    parser.add_argument("--k", type=int, default=10)
    parser.add_argument(
        "--query-tokens",
        type=int,
        default=3,
        help="Words taken from a cue to form its query (fewer = harder)",
    )
    parser.add_argument("--seed", type=int, default=7)
    parser.add_argument(
        "--transcriber",
        default="",
        help="Registered transcriber name for the WER layer (empty = skip, or 'auto' to use the first)",
    )
    parser.add_argument("--data-dir", type=Path, default=DATA_DIR)
    parser.add_argument("--out", type=Path, default=RESULTS_DIR)
    parser.add_argument("--revision", default=FLEURS_REVISION)
    parser.add_argument(
        "--peak",
        type=float,
        default=0.5,
        help="Peak level every clip is normalised to (0 = leave the raw gain alone)",
    )
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    random.seed(args.seed)
    args.data_dir.mkdir(parents=True, exist_ok=True)
    started = time.time()

    import chunkr

    print(f"== corpus: {FLEURS_REPO} @ {args.revision[:10]}", file=sys.stderr)
    corpus = build_corpus(
        args.data_dir, args.revision, args.clips, args.gap_ms / 1000, args.peak or 1.0
    )
    print(
        f"   {len(corpus.gold)} utterances, {corpus.duration_s:.1f}s, "
        f"{len(corpus.gaps)} gaps of {args.gap_ms} ms",
        file=sys.stderr,
    )

    seams = []
    for window_s in args.seam_seconds:
        row = probe_seams(corpus, window_s)
        seams.append(row)
        print(
            f"   [seams {window_s:.0f}s] windows={row['windows']} "
            f"gap_hit={row['gap_hit_rate']:.3f} loud_seam={row['loud_seam_rate']:.3f} "
            f"speech_s={row['speech_s']}",
            file=sys.stderr,
        )

    captions = []
    for max_seconds in args.caption_seconds:
        row = evaluate_captions(
            corpus, caption_windows(corpus, max_seconds), args.k, args.seed, args.query_tokens
        )
        row["splitter"] = f"subtitle:{max_seconds:.0f}"
        captions.append(row)
        print(
            f"   [{row['splitter']}] recall@{args.k}={row['temporal_recall@k']:.3f} "
            f"precision@{args.k}={row['temporal_precision@k']:.3f} "
            f"integrity={row['integrity']:.3f}",
            file=sys.stderr,
        )
    for label, windows in (
        ("fixed:30", fixed_windows(corpus, 30.0)),
        ("whole", whole_windows(corpus)),
    ):
        row = evaluate_captions(corpus, windows, args.k, args.seed, args.query_tokens)
        row["splitter"] = label
        captions.append(row)
        print(
            f"   [{label}] recall@{args.k}={row['temporal_recall@k']:.3f} "
            f"precision@{args.k}={row['temporal_precision@k']:.3f} "
            f"integrity={row['integrity']:.3f}",
            file=sys.stderr,
        )

    transcription: dict[str, Any] = {}
    name = args.transcriber
    if name == "auto":
        available = chunkr.transcribers()
        name = available[0] if available else ""
    if name:
        print(f"== transcription: {name}", file=sys.stderr)
        transcription["transcriber"] = name
        transcription["model"] = os.environ.get("CHUNKR_WHISPER_MODEL", "base (default)")
        runs = []
        for window_s in [None, *args.seam_seconds]:
            row = transcribe_corpus(corpus, window_s, name)
            runs.append(row)
            print(
                f"   [windows {window_s or 'off'}] wer={row['wer']:.3f} "
                f"per-utterance={row['wer_per_utterance']:.3f} segments={row['segments']} "
                f"missing={row['utterances_missing']} words={row['words']}",
                file=sys.stderr,
            )
        transcription["runs"] = runs
    else:
        print("   (no transcriber: WER layer skipped)", file=sys.stderr)

    payload = {
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "python": sys.version.split()[0],
        "settings": {
            "clips": args.clips,
            "gap_ms": args.gap_ms,
            "seam_seconds": args.seam_seconds,
            "caption_seconds": args.caption_seconds,
            "k": args.k,
            "query_tokens": args.query_tokens,
            "seed": args.seed,
            "transcriber": args.transcriber,
            "peak": args.peak,
        },
        "dataset": {"repo": FLEURS_REPO, "revision": args.revision, "license": "CC BY 4.0"},
        "corpus": {
            "utterances": len(corpus.gold),
            "duration_s": corpus.duration_s,
            "speech_s": corpus.speech_s,
            "speech_ratio": corpus.speech_s / corpus.duration_s,
            "gaps": len(corpus.gaps),
        },
        "seams": seams,
        "captions": captions,
        "transcription": transcription,
        "elapsed_s": round(time.time() - started, 1),
    }
    args.out.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    json_path = args.out / f"audio_fleurs_{stamp}.json"
    json_path.write_text(json.dumps(payload, indent=2))
    report = markdown(payload)
    json_path.with_suffix(".md").write_text(report + "\n")
    print(f"\nwrote {json_path}", file=sys.stderr)
    print(report)
    return 0


def markdown(payload: dict[str, Any]) -> str:
    corpus = payload["corpus"]
    lines = [
        f"Corpus: {corpus['utterances']} real utterances, {corpus['duration_s']:.1f}s, "
        f"{100 * corpus['speech_ratio']:.0f}% speech, {corpus['gaps']} silent gaps of "
        f"{payload['settings']['gap_ms']} ms "
        f"(`{payload['dataset']['repo']}` @ {payload['dataset']['revision'][:10]}).",
        "",
        "| seams (`CHUNKR_AUDIO_WINDOW_S`) | windows | in a gap | seam in a clip | loud seam | median below speech | speech_s | speech_ratio |",
        "| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for row in payload["seams"]:
        lines.append(
            "| {window_s:.0f}s | {windows} | **{gap_hit_rate:.3f}** | {seam_in_clip_rate:.3f} | "
            "{loud_seam_rate:.3f} | {median_seam_db_below_speech:.1f} dB | {speech_s} | "
            "{speech_ratio} |".format(**row)
        )
    lines += [
        "",
        "| caption splitter | windows | mean window | temporal recall@k | temporal precision@k | integrity |",
        "| :--- | ---: | ---: | ---: | ---: | ---: |",
    ]
    for row in payload["captions"]:
        lines.append(
            "| `{splitter}` | {windows} | {mean_window_s:.1f}s | **{temporal_recall@k:.3f}** | "
            "{temporal_precision@k:.3f} | {integrity:.3f} |".format(**row)
        )
    transcription = payload.get("transcription") or {}
    if transcription.get("runs"):
        lines += [
            "",
            f"Transcription ({transcription['transcriber']}):",
            "",
            "| windows | segments | WER | per-utterance WER | worst | utterances with no words | words | monotonic | inside media |",
            "| :--- | ---: | ---: | ---: | ---: | ---: | ---: | :--- | :--- |",
        ]
        for row in transcription["runs"]:
            lines.append(
                "| {window} | {segments} | **{wer:.3f}** | {wer_per_utterance:.3f} | "
                "{wer_worst:.3f} | {utterances_missing}/{utterances} | {words} | {mono} | "
                "{inside} |".format(
                    window="off" if row["window_s"] is None else f"{row['window_s']:.0f}s",
                    segments=row["segments"],
                    wer=row["wer"],
                    wer_per_utterance=row["wer_per_utterance"],
                    wer_worst=row["wer_worst"],
                    utterances_missing=row["utterances_missing"],
                    utterances=row["utterances"],
                    words=row["words"],
                    mono="✅" if row["timing_monotonic"] else "❌",
                    inside="✅" if row["timing_inside_media"] else "❌",
                )
            )
    return "\n".join(lines)


if __name__ == "__main__":
    raise SystemExit(main())
