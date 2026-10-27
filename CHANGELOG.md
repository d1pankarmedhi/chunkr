# Changelog

All notable changes to `chunkr` (Rust crate) and `chunkr-rs` (PyPI package) are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.7.0] - 2026-10-27

A reach release: the same engine now ingests what corpora actually arrive as — archives,
compressed files, more office and mail formats, spreadsheets-as-files, audio — without changing the
unit boundaries or the cite-back metadata that already existed. Every claim below is backed by a
benchmark in `benchmarks/`, including the two bugs the audio harness found on its first run.

Ships with **`chunkr-audio` 0.1.1** (word timings reach documents; first PyPI publication),
**`chunkr-tabular` 0.1.0** (first publication) and **`chunkr-pdf` 0.1.1** (the >1000-page truncation
fix). All three extensions are versioned independently of the base package and are now published by
the release workflow, so `pip install "chunkr-rs[audio]"` / `[tabular]` / `[pdf]` resolve. The
published crate is 1.1 MiB instead of 19.6 MiB: the benchmark cache and root-level scratch PDFs are
excluded from the package.

### Fixed

- **Speech detection no longer declares quiet recordings silent.** The energy VAD's absolute
  floor was -50 dBFS, so any recording more than ~25 dB down — a quiet mic, a phone call, a
  normalised-down corpus — was reported as `speech_detected: false` / `speech_ratio: 0.0` even
  though the speech is plainly audible. Measured on real `google/fleurs` utterances (peaks span
  0.005 to 0.96): **9 of 20 clips detected before, 20 of 20 after**, median `speech_ratio` 0.00 →
  0.83. The floor is now -80 dBFS, where only dither lives; the relative threshold above the noise
  floor is what separates speech from silence, and refusal still keys on near-silence
  (`SILENCE_PEAK`), so a file that is genuinely silent is still refused before a model loads.
- **PDF documents longer than 1,000 pages were silently truncated** by the `chunkr-rs[pdf]` /
  `chunkr-pdf` extension. The plugin passed `max_pages=None` through to liteparse, whose Python
  binding then fell back to its own 1,000-page default: a 2,066-page PDF came back as 1,000 pages,
  with no warning and no error, and every page past that point simply did not exist downstream.
  `scope.max_pages=None` now means every page, matching the documented behaviour and the Rust
  backend. The payload also reports the document's `total_pages` now. Regression tests cover the
  default, the explicit override, and (opt-in, `CHUNKR_BIG_PDF=<pdf>`) a >1000-page document.
- **`.csv` and `.tsv` now load as tables, as documented.** The dispatcher sent only
  `xlsx`/`xls`/`xlsb`/`ods` to the spreadsheet loader; delimited files fell through to the text
  fallback and came back as raw text, even though the format table promised "worksheet (as a Markdown
  table)". A new `src/loader/delimited.rs` parses them dependency-free and quote-aware — embedded
  delimiters and newlines stay inside a cell, `""` unescapes, CRLF and BOMs are handled, unquoted
  quotes are tolerated — and renders through the same Markdown renderer as the spreadsheets and the
  table-source extensions, so a CSV document carries `format: "table"`, `table_format`, `row_count`,
  `column_count` and `row_offset`. Found by the new retrieval benchmark (below), where a whole table
  corpus retrieved identically to raw text.

### Added

- **An audio benchmark, and the two bugs its first run found.** `benchmarks/bench_audio.py` builds a
  corpus from real `google/fleurs` utterances (streamed from the pinned revision, peak-normalised,
  joined with known silent gaps) and measures what text metrics cannot: seam placement in seconds
  (no seam lands on loud speech, median 27 dB below the surrounding speech), VAD accounting against
  the known speech duration (`speech_ratio` 0.93 vs gold 0.95), caption-window retrieval with exact
  temporal gold (integrity 1.000 for `SubtitleChunker` vs 0.683 for a naive 30 s timeline; temporal
  precision 0.844 → 0.016 as windows grow), and — with any registered transcriber — word error rate
  with windowing off/on (0.094 → 0.101 at 20 s, 0.109 at 30 s), plus monotonicity and in-media
  timestamp checks. The VAD found quiet recordings to be "silent" (see Fixed) and the harness's own
  first WER run was measuring clip gain rather than chunking, both of which are documented in
  `benchmarks/README.md`.
- **Retrieval benchmark now reports precision, and proves the archive paths are retrieval-neutral.**
  `benchmarks/bench_retrieval.py` gains `P@k` (share of the top-k chunks that carry the answer) and
  `doc P@k` (share taken from the gold document), so recall and precision are read together, plus
  `--containers gz|tar-gz|7z` and `--suite`: every corpus is re-materialised as per-document `.gz`,
  `.tar.gz`, `.7z` and as one corpus-wide `.tar.gz` (doc id from `archive_entry`) and compared
  against the plain files. All 16 container runs are parity-EXACT on chunks, hit@1/3/5/10, MRR,
  integrity, nDCG, R@10 and both precision figures, including a 5,183-member archive.
- **Archives beyond zip: `7z`, `tar`, compressed tar and single compressed files.** Every container
  is recognised by its magic bytes, so extensionless and mislabelled payloads expand too: `tar`,
  `.tar.gz`/`.tgz`, `.tar.bz2`, `.tar.xz`, `.tar.zst`, `7z`, and a compressed *document*
  (`.csv.gz`, `.jsonl.bz2`, `.xml.xz`, `.txt.zst`) which loads as one document named by its inner
  name. Members go through the normal extractors as before and keep `archive_entry` /
  `archive_entries` / `archive_skipped`; `source` shows the whole chain
  (`export.tar.gz!deep/notes.txt`). `rar` is recognised and refused with advice — there is no
  pure-Rust reader for it and this crate takes no C dependency. Every decoder (flate2/miniz_oxide,
  bzip2-rs, lzma-rust2, ruzstd, sevenz-rust2, tar) is pure Rust, so nothing new is needed on wasm.
- **Decompression limits for streams that declare no size.** zip checks each entry's declared
  sizes, 7z and tar check the header sizes before reading, and gz/bz2/xz/zst are cut off mid-stream:
  a 200 MB gzip bomb is refused in milliseconds instead of being expanded. A compressed tar counts
  as *one* archive for `max_depth`, and `ArchiveLimits` now propagates into nested archives instead
  of being reset to the environment defaults partway down the tree. `expand_zip` became `expand`,
  which takes any supported container.
- **Word timings survive transcription, and long audio can be cut before the model sees it.**
  `TranscriptSegment` carries `words` (per-word `text`/`start`/`end`) — accepted from Python
  transcribers as `{"word", "start", "end"}`, and read from whisper.cpp's `-ojf` token offsets by the
  CLI bridge, which now merges sub-word pieces and trailing punctuation into words and drops special
  tokens. Each document gains flat `word_count`, `first_word_s` and `last_word_s`, so a citation can
  point at a word and a long segment can be re-cut without losing its timestamps.
  `CHUNKR_AUDIO_WINDOW_S=<seconds>` cuts the payload into non-overlapping windows with every seam at
  the quietest frame near the boundary (WhisperX's min-cut; nothing is dropped), transcribes each and
  restores the original timeline, reporting `audio_windows`. Off by default: backends that batch
  already window themselves. Measured on 27 s of real speech, 20 s windows returned the same 50 words
  as a single call.
- **Retrieval-quality benchmark over Hugging Face datasets** (`benchmarks/bench_retrieval.py`,
  repository tooling, not shipped): SQuAD answer spans, SciFact and NFCorpus BEIR qrels, and
  WikiTableQuestions gold rows, materialised as real `.txt`/`.csv` files and retrieved with BM25, so
  chunkers are compared on evidence containment, boundary integrity and nDCG rather than on a proxy.
  Corpora are pinned to the dataset commit each run resolves; the run is one minute and needs no
  model download.
- **Speech detection: noise gate, and a refusal that can no longer be wrong.** Frames whose
  zero-crossing rate says "broadband noise" are not counted as speech, so a hiss or a fan no longer
  reports 100% speech. More importantly, `load_documents` now refuses audio only when it is actually
  silent (peak below -54 dBFS) instead of when the energy estimate found no spans: measured on real
  speech mixed with white noise, that estimate reports **nothing at 0 dB SNR**, which used to deny a
  recording Whisper transcribes correctly. Noisy files now go through with
  `speech_detected: false` and `speech_ratio: 0.0`, and the transcriber's own verdict stands. No
  Silero, no ONNX runtime, no C dependency — the reasoning, with the measurements, is in
  `src/audio/vad.rs`.

- **Speech models: pinned manifest, verified on-demand download, `chunkr models`** (`--features audio`).
  Weights are not bundled — crates.io caps a crate at 10 MB, a published wheel would carry a model
  most users never call, and choosing a model is the caller's decision about language, latency and
  hardware. What ships is the manifest (`tiny.en`, `base.en`, `small.en` and the multilingual trio,
  each with its size and SHA-256 as published upstream) plus `chunkr models list|pull|dir` and
  `--asr <model>`: a run downloads what it needs into the platform cache (`CHUNKR_MODEL_DIR`
  overrides), writes through `.part`, verifies size and digest, then renames — so an interrupted
  download can never masquerade as a model. `--offline` refuses to fetch. Downloading goes through
  `curl`, `wget` or PowerShell rather than a TLS stack: the crate keeps its no-C-dependency promise
  and no HTTP client ships for a code path most users run once.
- **`--asr` turns the CLI into a transcriber with no configuration**: it resolves the model, finds
  `whisper-cli`/`whisper-cpp` on `PATH` (or `CHUNKR_WHISPER_BIN`), and registers the backend for the
  run, so `chunkr episode.mp3 --asr base.en` is the whole setup.
- **`ffmpeg` containers are advertised when an `ffmpeg` is configured**: `opus`, `webm`, `mkv`, `mp4`,
  `m4v`, `mov`, `avi`, `aiff`, `wma`, `amr`, `caf` join the audio family, which means the audio track
  of a lecture or call recording transcribes into the usual timestamped chunks. They are listed only
  while `CHUNKR_FFMPEG` is set, so `supported_extensions()` never promises what the process cannot do.
- **Outlook `.msg`**: CFB property streams, RTF-compressed bodies and attachments via `msg_parser` —
  one document with `format="outlook"`, `subject`, `from`, `to`, `cc`, `date`, `msg_class`,
  `attachment_count` and `attachments`, mirroring what the EML loader produces so a mailbox exported
  either way chunks and filters the same. Attachments are dispatched by their own extension like zip
  members — a forwarded spreadsheet chunks as a table, a PDF as pages, an embedded `.msg` as a
  message (bounded to 4 levels) — each tagged `attachment_name`/`attachment_index` with a chained
  `source` (`mail.msg!Test Email.msg`); unreadable ones (images, legacy `.doc`) are skipped rather
  than faked.
- **Speaker-aware windows**: when cues carry speakers (WebVTT voice tags, diarized transcripts) a
  window never mixes two of them — a chunk that reads "yes, absolutely — no, never" with no
  attribution is worse than two shorter attributed chunks. Windows record `speaker`, or `speakers` +
  `speaker_turns` when an unlabelled cue let two voices through.
- **`CHUNKR_ARCHIVE_DEPTH` / `CHUNKR_ARCHIVE_ENTRIES` / `CHUNKR_ARCHIVE_MB`** raise the archive limits
  from the environment for pipelines whose exports are zip-of-zip, with zip-bomb guards intact.

- **Audio: timestamped transcripts behind the `audio` feature** (`chunkr-rs[audio]`). Audio files
  (`wav`, `mp3`, `flac`, `ogg`, `oga`, `m4a`, `m4b`, `aac`) now load through the format dispatcher:
  the container is decoded in pure Rust (symphonia — no system codecs, no C dependency), the speech
  is measured, and a registered `Transcriber` turns it into segments. Each segment becomes a
  `Document` with `start_time_s`/`end_time_s`/`speaker` plus `format="audio"`, `audio_format`,
  `audio_duration_s`, `sample_rate`, `channels`, `speech_s`, `speech_ratio`, `transcriber`,
  `segment_index` and `total_segments`, so `SubtitleChunker` merges them into time windows that can
  be cited back to a timestamp and `DirectoryLoader` routes audio by time rather than by character
  count.
- **Opus decodes through an opt-in `ffmpeg` bridge** (`CHUNKR_FFMPEG`). symphonia implements no
  Opus decoder, and Opus is what WhatsApp voice notes, browser recordings and most downloaded audio
  actually contain, so the extension alone promised more than the core could do. Codecs the built-in
  path cannot handle now name themselves (`unsupported audio codec 'opus'` instead of
  `unsupported codec`) and, when `CHUNKR_FFMPEG` points at a binary, decode through it — nothing
  runs unless the pure-Rust path has already failed.
- **CLI transcripts from a local whisper.cpp** (`CHUNKR_WHISPER_BIN` + `CHUNKR_WHISPER_MODEL`):
  the core resamples to the 16 kHz mono WAV the binary requires, runs it with `-oj`, and maps its
  segments — so `chunkr episode.mp3` produces timestamped chunks with no Python and no model in the
  crate.
- **`resample`**: band-limited downsampling (averaging before decimation, so 44.1 → 16 kHz does not
  fold high frequencies into the speech band), exposed because every model wants 16 kHz and
  containers deliver whatever they were encoded at.
- **Transcriber registry** (`register_transcriber` / `unregister_transcriber` /
  `registered_transcribers`, and the same three from Python): models are plugged in like PDF parsers
  and table extractors, so the core keeps no ASR dependency. The most recent registration wins, and
  `CHUNKR_TRANSCRIBER` picks between several by name.
- **`packages/chunkr-audio`**: faster-whisper backend, discovered through the
  `chunkr.transcribers` entry point. The model is built on first transcription, never at import, and
  `chunkr.register_transcriber` accepts any callable returning
  `{"start", "end", "text", "speaker"?}` — a hosted API or in-house model works the same way.
- **Energy VAD** (`speech_spans`, `VadConfig`): an adaptive noise floor plus absolute floor,
  hangover merging and short-blip rejection, with no model artifact to ship. It reports speech
  timing, makes a silent file fail *before* a model is called, and fills `speech_ratio`. A recording
  with no quiet frame anywhere is treated as speech rather than silence, so the transcriber decides.
- **Number display formats in worksheets**: percentages, currency and thousands separators now
  render the way the spreadsheet shows them (`0.15` with `0%` becomes `15%`, `1234.5` with
  `"$"#,##0.00` becomes `$1,234.50`, `#,##0` becomes `1,235`), including custom formats declared in
  `xl/styles.xml`, negative and zero sections (`($1,234.50)`, a literal `"nil"` section) and locale
  currency brackets (`[$€-407]`). calamine exposes values but drops cell styles, so the style table
  and each cell's `s=` attribute are read directly. Rounds halves away from zero like Excel. Covers
  xlsx only; xls/xlsb/ods keep stored values, and builtin locale currencies render with `$`.
- **The CLI keeps the unit each chunk came from**: a file is chunked per extracted unit (page,
  sheet, slide, chapter, email, archive member, subtitle window, JSONL record span) and each chunk
  inherits that unit's metadata, so `--format jsonl` now carries `sheet_name`, `page_number`,
  `start_record`, `start_time_s` and friends instead of discarding them. Text output prints a
  `=== report.xlsx · sheet Budget 1/2 ===` header whenever the unit changes, which also makes
  `--strategy dir` output unambiguous.

### Fixed

- **Audio duration came from the container header, not the decoded samples.** A WAV streamed with an
  unknown data size declares ~2^32 frames and reported 12 hours for a 4-second file; every timestamp
  derived from it was wrong. Duration is now measured from the samples that were actually decoded,
  which is also the audio a model was given.
- **Transcriber spans past the end of the media are clamped.** Whisper pads its last window to a
  frame boundary and can report an end beyond the file (a 13.0 s clip ended at 13.1 s); a chunk
  citing a timestamp the file does not have is worse than a shortened span.
- **Worksheets whose used area does not start at A1 were read as empty.** Cell lookups used grid
  positions against calamine's absolute-position API, and merge/style coordinates were applied to
  grid indices; a sheet whose first used cell is `C3` lost its header, its merges and its number
  formats. Values, formulas, merges and formats now share one absolute frame.

### Added

- **Zip archives expand member by member**: `.zip` is now a supported extension and each member is
  dispatched through the normal extractors by its own extension, so a `.csv` inside an archive is
  chunked as a table (headers repeated) and a `.md` as Markdown rather than everything being
  flattened to text. Documents carry `archive_entry` (path inside the archive), `archive_entries`
  and `archive_skipped` counts, and keep the member's own `file_extension`/`file_name` while
  `source` shows the full chain (`bundle.zip!data/sales.csv`); `DirectoryLoader` routes members by
  the format their extraction reported. Members that are not readable documents (images, binaries,
  macOS resource forks) are skipped and counted instead of failing the archive.
- **Zip-bomb limits, enforced before an entry is read** (`ArchiveLimits`, defaults: depth 2,
  1000 entries, 512 MiB uncompressed, 100:1 per-entry ratio above a 1 MiB floor). A breach reports
  the limit it hit (`archive: entry 'bomb.txt' expands 900x, over the 100x limit`) instead of
  allocating it; nested archives past the depth limit and corrupt archives error with the offending
  entry named. `expand_zip` and `ArchiveLimits` are public for callers that want tighter budgets.
- Extensionless payloads that are zip archives now expand instead of erroring as unknown binaries,
  since archives are a supported format rather than a guess.
- **Worksheet quality: merged regions, titles, multi-level headers, real dates**. Spreadsheet
  extraction is now structural instead of naive:
  - Merged regions are filled, so a category spanning rows labels every row it covers and a header
    merged across columns labels each of them. A value already in the region is never overwritten
    (xlsx and xls report merges; xlsb and ods do not expose them).
  - A title row above the header (`Report 2024` in a cell of its own, or merged across the table)
    becomes `sheet_title` metadata and leaves the header detection alone.
  - A header merged over several rows becomes a multi-level header — `2024 / Q1` — with
    `header_rows` recorded, instead of shifting the first data row into the header.
  - Date and duration cells render as text (`2023-03-15`, `36:00:00`) instead of Excel serial
    numbers; a cell whose cached value is missing falls back to its formula (`=B3*3`) so workbooks
    saved without cached values still say something. The formula pass is skipped on dense sheets.
  - Trailing formatting-only columns are dropped, and `row_count` now counts data rows (the header
    is reported through `header_rows`/`column_count`).
  - Number *display* formats (percentages, currency, thousands separators) still come out as stored
    values: calamine classifies only dates and durations from `styles.xml`, so `0.15` cannot be
    told apart from `15%` without parsing the style table ourselves.
- **Magic-byte sniffing for payloads that do not name themselves**: an empty, wrong or unknown
  extension is no longer the end of the road. PDF (`%PDF-`) and RTF (`{\\rtf`) are recognised by
  prefix, and zip containers are attributed by their entries — `word/document.xml` → docx,
  `xl/workbook.xml` → xlsx, `ppt/` slides → pptx, `mimetype`/`META-INF/container.xml` → epub,
  `content.xml` + `META-INF/manifest.xml` → odt. Dependency-free (no mime database), and never a
  guess: a known text extension is authoritative, and an unattributable archive still reports
  `binary payload is not a supported format`.
- **Malformed JSONL is reported, not silently text-chunked**: `.jsonl`/`.ndjson` lines are validated
  while loading, so a truncated or corrupt corpus fails with `line <n>` and the source path instead
  of quietly losing record spans. `.json` keeps its tolerant path (comment-carrying and JSON5-ish
  documents still load), matching the difference between a machine-generated line format and a
  hand-edited document.
- **Table-source registry: optional tabular extractors (`chunkr-tabular`)**. Core has no columnar
  dependency; an extension registers a `TableSource` that extracts rows as plain strings, while
  `documents_from_tables` keeps the mapping in core — Markdown rendering (now shared with the
  spreadsheet loader, so output is identical), cell escaping, metadata (`format: "table"`,
  `table_format`, `table_name`, `table_index`, `total_tables`, `row_offset`, `row_count`,
  `column_count`) and spans. Registered extensions join `supported_extensions()` / `is_structured()`
  and `DirectoryLoader` routes them to row-based table chunking. Rust crates implement `TableSource`
  directly; Python plugins register a callable via `chunkr.register_table_loader(name, extensions,
  loader)` (plus `unregister_table_loader` / `table_loaders`), discovered from the
  `chunkr.table_loaders` entry-point group (`CHUNKR_NO_TABLE_PLUGINS=1` opts out). A path is passed
  when the caller has one, so an extractor can stream instead of buffering the file.
- **`packages/chunkr-tabular`**: the first extension package — Parquet and Arrow IPC (`parquet`,
  `feather`, `arrow`, `ipc`) read with pyarrow and handed over batch by batch, with `row_offset`
  tracking rows across batches. `DirectoryLoader` releases the GIL while a directory loads, so
  Python-backed extractors can run on worker threads instead of deadlocking.
- `supported_extensions()` now returns `Vec<String>` (it can include registered extensions) and
  `ChunkPacker` reconciles `table_index`/`table_name`/`row_offset` spans while summing `row_count`.
- **JSONL/NDJSON support in `JsonChunker`**: line-delimited JSON — the format embedding corpora
  usually ship in — is no longer split mid-object by text chunking. Whole records are batched into
  chunks tagged with `path` (`$[0..250]`), `start_record`/`end_record` (0-based, inclusive, indexing
  the outermost record sequence), `is_jsonl`, and a record larger than the budget is descended into
  by field while keeping its record index. Blank lines are skipped (record indices stay dense) and
  CRLF is normalized. Detection is strict: every non-blank line must be one JSON value, so prose,
  source code with lone braces and pretty-printed JSON never look line-delimited — a payload that
  only partly parses falls back to `RecursiveChunker` instead of inventing record spans.
- Record spans on JSON arrays: batches of top-level array elements now carry `start_record`/
  `end_record` too, elements descended individually keep their index, and oversize scalars split by
  `RecursiveChunker` still report the `path` they came from. Root-level paths are rendered as
  `$[...]` instead of `[...]`, matching the single-document `$` case.
- `jsonl` and `ndjson` are dispatched by `load_documents`/`supported_extensions()` and routed by
  `DirectoryLoader` to `JsonChunker`. Documents loaded from `json`, `jsonl` and `ndjson` now carry
  `format: "json"` instead of `"text"`, so the family is filterable the same way as
  `subtitle`/`spreadsheet`/`epub`.
- `ChunkPacker` span reconciliation now also covers `path`, `start_record` and `end_record`, so
  packed JSON chunks cannot inherit another source's record range. Counted metadata (`cue_count`) is
  summed over merged sources instead of inherited, and the merged chunk keeps the summed value.
- **Time-indexed chunking for caption tracks — `SubtitleChunker`**: SRT and WebVTT load one
  `Document` per cue carrying `start_time_s`/`end_time_s`/`duration_s`/`cue_index`/`total_cues` and,
  for WebVTT voice tags, `speaker`. `SubtitleChunker` merges consecutive cues into time windows and
  emits `start_time_s`, `end_time_s`, `duration_s`, `cue_count` and a `speaker` when the window has
  one, so retrieval can cite the exact moment in the media file. Cues are atomic — a cue is never
  split, even when it alone exceeds a budget — and `DirectoryLoader` routes `.srt`/`.vtt`/`.webvtt`
  to it. `Cue`/`parse_cues` are public for callers that want the parse without the windowing.
- `ChunkPacker`'s span reconciliation now covers `start_time_s`, `end_time_s`, `duration_s` and
  `speaker`: packing chunks with different spans drops the singular key and records
  `merged_<key>` instead of letting a merged chunk claim one source's span.
- **One extraction dispatcher for every supported format**: `load_documents`,
  `load_documents_from_bytes` and `supported_extensions` are exported from the Rust prelude, the
  Python module (`chunkr.load_documents`, `chunkr.load_documents_from_bytes`,
  `chunkr.supported_extensions`) and the CLI. Extracted formats cover PDF (`pdf`),
  spreadsheets (`xlsx`, `xlsm`, `xlsb`, `xls`, `ods`), office documents (`docx`, `docm`, `pptx`,
  `odt`), books and mail (`epub`, `eml`, `mbox`), markup and notebooks (`html`, `xml`, `rtf`,
  `ipynb`), captions (`srt`, `vtt`, `webvtt`) plus every text format, returning one `Document` per
  page, worksheet, slide, chapter, message or cue with `sheet_name`, `slide_number`,
  `chapter_href`, `page_number`, `start_time_s`, `source` and `format` metadata attached.
- `DirectoryLoader` (`load_files`, `load_and_chunk`, and their `_lenient` variants) and the CLI now
  extract through that dispatcher instead of `read_to_string`, so structured formats are usable in
  batch ingestion rather than failing with `stream did not contain valid UTF-8`. Spreadsheets route
  to `TableChunker`, so the header row repeats on every chunk, and extraction errors are prefixed
  with the offending path.
- Text payloads follow one rule: a byte-order mark is always honoured, `.txt`/`.log`/`.csv`/`.tsv`
  additionally accept legacy single-byte encodings, and formats that are UTF-8 by convention
  (`md`, `json`, `xml`, source code) reject undecodable bytes instead of silently producing mojibake.
- `benchmarks/pdf_quality.py`: measures what each PDF extractor preserves (consensus recall,
  word precision, junk characters, engine error text) and what retrieval ends up with (chunks
  polluted by junk, chunks with no sentence), aligning pages by page number. It is what exposed
  the truncation above and the Identity-H failure mode of the built-in lopdf extractor.
- `benchmarks/bench_pdf.py` now benchmarks the extension's liteparse backend alongside the
  built-in fast path, PyMuPDF and pypdf, and labels `PDFLoader()`'s `auto` resolution explicitly.

---

## [1.6.0] - 2026-10-10

### Added
- **Optional high-fidelity PDF parsing — `chunkr-rs[pdf]` / `chunkr-pdf`**: a separate extension package (and the `pdf` Cargo feature for Rust) that extracts PDFs with layout awareness — reading order, headings, lists, tables, figures, page labels, per-page complexity signals and optional OCR — then maps the result to `Document`s inside chunkr, so both paths share one implementation.
- **`chunkr::parser` module (base crate, no new dependencies)**: `ParserConfig` with presets (`retrieval`, `faithful`, `structure`) and every knob user-selectable (output `text`/`markdown`/`both`, granularity `page`/`block`/`document`, `extract` toggles, `sanitize` rules + thresholds, OCR mode, page scope, error policy), plus `pages_to_documents`, `render_markdown`, `sanitize_pages`, typed `PagePayload`/`BlockPayload` and `PdfParser` backend dispatch (`auto`/`fast`/`liteparse`).
- **Configurable sanitizer** for backend layout blocks: table rejection (`require_header`, `header_missing` `demote`/`synthesize`/`keep`, `max_words_per_cell`, `prose_cell_ratio`, `min_rows`, `min_columns`, `demote_to`) and heading rejection (`max_len`, `drop_truncated`, levels `auto`/`as_is`/`flat`/`derive`), glyph repair (`repair`/`report`/`off`) and a junk-glyph guard (`flag`/`fallback_fast`/`off`). Every change is reported in `sanitize_report` metadata; `sanitize.enabled = false` is a byte-for-byte passthrough.
- **Python plugin API**: `chunkr.ParserConfig`, `chunkr.pages_to_documents`, `chunkr.register_pdf_backend` / `unregister_pdf_backend` / `pdf_backends`, and `chunkr.PDFLoader(backend=..., config=...)` — `backend="auto"` uses an installed backend automatically, a name resolves a registered one, a callable is used inline.
- **`chunkr-pdf` package**: `PDFParser` with presets and full config passthrough, `load`/`load_pages`/`load_document`/`text`/`blocks`/`stream` (bounded-memory page batches)/`is_complex`/`payload`, and a `chunkr.pdf_backends` entry point so `pip install "chunkr-rs[pdf]"` is enough. Requires Python 3.10+ (bumped by the optional dependency, not by the base package).

- **Pluggable OCR backends**: `ocr.backend` picks the engine — `tesseract` (bundled), `ppocr` (reserved for the in-process ONNX build), `server` (any service speaking liteparse's OCR API), or the name of an engine registered with `chunkr.register_ocr_backend(name, engine)`. Python engines are served to the backend through a loopback HTTP server, so reading order, rotation handling and text-layer merging stay in one place; `chunkr_pdf` ships ready adapters for RapidOCR (`chunkr-pdf[ocr-rapid]`), PaddleOCR (`[ocr-paddle]`), EasyOCR (`[ocr-easyocr]`) and Surya (`[ocr-surya]`) — the first three verified end-to-end against a rasterised fixture page, Surya matched to its released API and unit-tested with its block shape (its VLM needs a separate `llama.cpp`/`vLLM` runtime), plus `ocr.server_url`/`ocr.headers`/`ocr.hedge_delays_ms` for remote services. Rust embedders can inject any engine with `PdfParser::with_ocr_engine(Arc<dyn OcrEngine>)`.
- **`ocr.mode="auto"` is now a real per-page gate**: instead of OCRing every page, the document is parsed once without OCR, `chunkr.pages_needing_ocr`/`chunkr::parser::select_ocr_pages` selects the pages whose text layer is broken (`scanned`, `no-text`, `garbled` by default; `ocr.auto_reasons`/`ocr.auto_min_chars` to override), and only those are re-parsed with OCR and spliced back in. On a 16-page digital deck this performs zero OCR work; on the textbook fixture it OCRs only the pages that need it.
- **OCR config validation and language handling**: `ParserConfig::validate()` / `ParserConfig(...)` now reject unusable setups at construction (a `server` backend without `server_url`, an unknown PP-OCR preset or device, a zero DPI, a malformed `scope.target_pages`, `concurrency`/`timeout_ms` of 0) and `ocr_language`/`chunkr.ocr_language` convert between Tesseract (`eng`) and ISO 639-1 (`en`) codes per engine.

- **In-process ONNX PP-OCR** for Rust users: `Cargo.toml` features `pdf-ocr-ppocr` (plus `-coreml`, `-cuda`, `-directml`, `-openvino`, `-tensorrt`, `-webgpu`) wire liteparse's `oar-ocr` engine to `ocr.backend="ppocr"`, with `ocr.ppocr.preset` (`tiny`/`small`/`medium`), `models_dir` for offline caches and `device` validated against the compiled accelerator. Models auto-download into `$OAR_HOME` and are SHA-256 verified; engines are cached process-wide, and `mode="auto"` only builds one after the gate actually selects a page. Verified end-to-end: rasterised fixture page OCRed and merged in 0.20 s warm (release), 30 parser tests green with the feature on.

- **Page-level parser backends**: `chunkr_pdf.parsers` adds `docling(url)` (docling-serve `/v1/convert/file`, structured output grouped by page), `mistral(api_key)` (`/v1/ocr`, page markdown) and `vlm(base_url, model)` (any OpenAI-compatible vision model — olmOCR, Qwen-VL, dots.ocr, vLLM — with pages rasterised through liteparse). Register one and `chunkr.PDFLoader(backend="docling")` uses it; markdown is split into headings/list items/paragraphs for structured output and kept verbatim for `sanitize={"enabled": False}`.

### Changed
- **Python package layout**: the native extension now ships as `chunkr._core` behind a thin `chunkr/__init__.py` that re-exports it and discovers PDF backends. `import chunkr` and every existing API are unchanged; `chunkr.pyi`/`py.typed` moved to `python/chunkr/`.
- **Cargo feature `pdf`** (and `pdf-ocr` for the bundled Tesseract engine) — off by default, non-wasm; enabling it adds the `liteparse` dependency.

### Fixed
- Nothing yet.

---

## [1.5.0] - 2026-10-07

### Changed
- **`RecursiveChunker` defaults are now tuned for retrieval accuracy**: the sentence-aware hierarchy (`SENTENCE_SEPARATORS`, paragraph -> line -> sentence -> word, also exposed as `chunkr.SENTENCE_SEPARATORS`) with overlap 120 instead of 200. On Chroma's token-level benchmark that is 0.792 recall and 0.255 `prec_Ω` versus 0.784 and 0.213 before — better on every metric at once, with 6% fewer chunks. `MarkdownChunker` inherits the same sub-chunking. Pass `separators=["\n\n", "\n", " ", ""]` and `overlap=200` for the previous boundaries.

### Fixed
- **`RecursiveChunker` could emit chunks larger than `chunk_size`**: the overlap carried into the next chunk was not re-checked against the cap, so chunks could reach `chunk_size + overlap` bytes (for example 1126 bytes with `chunk_size=1000` and `overlap=200`). The carried overlap is now trimmed until the next piece fits.

---

## [1.4.0] - 2026-09-05

### Added
- **Lenient batch loading**: `DirectoryLoader::load_files_lenient` and `load_and_chunk_lenient` (Rust + Python) return `(documents, errors)` instead of failing the whole batch on one bad file; `DirectoryLoader` skips symlinks and reports path-qualified IO errors.
- **Semantic context window**: `SemanticChunker::with_buffer_size` builder (Rust) and `buffer_size` parameter (Python, default `1`) — each sentence is embedded together with neighboring-sentence context for stabler breakpoints (`0` restores legacy isolated embedding).
- **`ChunkSpans`**: public type alias for the `(document, token-span)` pairs returned by `LateChunker::chunk_spans`, re-exported in the prelude.

### Changed
- `tiktoken-rs` 0.6 → 0.12: identical BPE tables, faster encoding (TokenChunker BPE throughput up, `to_vec` clone removed).

### Fixed & Improved (chunking efficiency + robustness)
- **Efficiency**:
  - `TokenChunker` now shares BPE tables via `Arc` — `clone()` and legacy `chunk_text()` no longer rebuild rank tables from embedded data.
  - `LateChunker` caches per-token decoded byte lengths by token id (distinct tokens << total tokens on natural text).
  - `RecursiveChunker` separator probing uses a single `memmem::Finder::find` scan per candidate; char fallback streams `char_indices` without a per-char index `Vec`.
  - `CharacterChunker` resolves byte offsets only at chunk boundaries (O(chunks) memory, two allocation-free passes).
  - `SentenceChunker` abbreviation guard is allocation-free and bounded to the last token (was `split_whitespace` + `to_lowercase` over the whole prefix per `.`).
  - `FastLexicalEmbedder` embeds sentences in parallel via Rayon and hashes words allocation-free; cosine distance is now true normalized cosine, safe for custom unnormalized embedders.
  - `StreamChunker` tracks a read offset with bulk compaction instead of `String::drain` per chunk (was O(n²) memmoves on multi-GB streams).
- **Robustness**:
  - `TokenChunker`, `HFTokenChunker`, `PropositionChunker`, `QueryAwareChunker` re-validate `chunk_size`/`overlap` in `chunk()` so post-construction mutation of public fields returns `InvalidChunkSize`/`InvalidOverlap` instead of underflow panic/hang.
  - `MarkdownChunker` honors `include_header_in_content = false` (previously a dead flag) and detects headers under CRLF line endings.
  - CLI sentence/paragraph overlap values are clamped instead of erroring on defaults; `Contextual`/`Hierarchical`/`AstCode` legacy-path configs preserved.
  - Research grounding: late-chunking flow follows Günther et al. (2024, arXiv:2409.04701) — full-text token stream + mean-pool over spans; delimiter fast-path rationale per memchr/SIMD backward-search analysis.

---

## [1.3.0] - 2026-09-04

### Added
- **WebAssembly Support (`wasm32-unknown-unknown`)**:
  - Full first-class compatibility for Browsers, Cloudflare Workers, Node.js, Deno, and Bun.
  - Pure Rust implementation on Wasm target with C-based tree-sitter gated to native platforms.
  - Sequential fallbacks on `wasm32` for Rayon parallel chunking (`par_chunk_documents`, `par_chunk_texts`, `par_enrich`).
- **WebAssembly Bindings (`chunkr-wasm`)**:
  - `wasm-bindgen` bindings for all chunking strategies: `RecursiveChunker`, `MarkdownChunker`, `TokenChunker` (OpenAI BPE `cl100k_base`, `o200k_base`, `p50k_base`, `r50k_base`), `CodeChunker`, `HtmlChunker`, `JsonChunker`, `TableChunker`, `SentenceChunker`, `ParagraphChunker`, `CharacterChunker`, `WordChunker`, `SemanticChunker`, `LateChunker`, `PropositionChunker`, `HierarchicalChunker`, `QueryAwareChunker`, and `StreamChunker`.
  - In-memory `PDFLoader`: `loadTextFromBytes`, `loadDocumentFromBytes`, and `loadPagesFromBytes` for zero-server in-memory PDF parsing and extraction directly at the edge.
  - `ChunkPipeline`: Composable post-chunking filtering, deduplication, packing, and SHA-256 metadata enrichment.
  - Native JavaScript objects serialization via `serde_wasm_bindgen::Serializer::json_compatible()`.
  - Top-level `chunk()` and `countTokens()` helper functions.
- **Universal NPM Package**:
  - Multi-target packaging in `wasm/`: `web` (Browsers & Cloudflare Workers), `bundler` (Vite, Webpack, Rollup), `nodejs` (Node.js CommonJS & ESM).
- **Examples**:
  - `examples/cloudflare-worker/`: Ready-to-deploy Cloudflare Worker demonstrating synchronous Wasm loading, text chunking, and in-memory binary PDF parsing.
  - `examples/browser/`: Interactive client-side HTML demo showing multi-strategy live chunking and performance benchmarks.
- **CI/CD Wasm Publishing**:
  - Automated Wasm compilation, testing, and NPM release jobs in `.github/workflows/publish.yml`.

---

## [1.2.1] - 2026-09-04

### Added
- **Automated GitHub Releases**: Integrated automated GitHub Release publishing with compiled platform wheels (`.whl`) and source distributions (`.tar.gz`) attached directly to releases.
- **Annotated Tag Title & Release Notes Extraction**: Workflow automatically extracts release titles and custom markdown notes directly from git annotated tag messages, with fallback to GitHub's auto-generated release notes.
- **Comprehensive Rustdoc Documentation**: Added crate-level (`src/lib.rs`) and module-level documentation across all chunker families (`src/chunker/mod.rs`), pipelines (`src/pipeline/mod.rs`), loaders (`src/loader/mod.rs`), and core structures (`src/structures/mod.rs`).
- **Standardized Changelog**: Added `CHANGELOG.md` tracking all release milestones.

### Changed
- Improved release guide in `PUBLISHING.md` with examples for annotated tags, release notes, and multi-platform publishing.

---

## [1.2.0] - 2026-09-04

### Added
- **StreamingChunker**: Constant-memory sliding-window chunking for processing multi-GB files, network sockets, and UNIX STDIN.
- **Ecosystem Bridges**: Zero-copy adapter functions `to_langchain`, `to_llamaindex`, `from_langchain`, and `from_llamaindex` for seamless integration into AI agent frameworks.
- **CLI Stream Ingestion**: `chunkr-cli` support for streaming large files and UNIX piping with `--strategy stream`.
- **Post-Chunking Pipeline**: Composable `ChunkPipeline` providing:
  - `ChunkFilter`: Threshold filtering by minimum/maximum character length, word count, and alphanumeric ratio.
  - `ChunkDeduplicator`: Exact content hashing deduplication.
  - `MetadataEnricher`: Deterministic SHA-256 chunk IDs, length metrics, and timestamps.
- **ChunkPacker**: Greedy bin-packing optimizer to merge small chunk fragments into token budget windows.
- **AstCodeChunker**: Tree-sitter AST syntax chunker splitting along function and class definitions for Rust and Python.
- **DirectoryLoader**: Recursive multi-threaded folder scanning with extension filtering and auto-routing to optimal chunkers.
- **HFTokenChunker**: Universal token-based chunking supporting any Hugging Face tokenizer (Llama 3, Mistral, Qwen, BERT, BGE).
- **TableChunker**: Structure-aware tabular chunking for Markdown, CSV, and TSV tables with automatic header duplication across chunks.
- **LateChunker**: Full-document context chunking with token span snapping and embedding mean-pooling.

---

## [1.1.0] - 2026-09-03

### Added
- **Multi-Core Parallelism**: Rayon-backed `par_chunk_documents` and `par_chunk_texts` executing parallel chunking across all available CPU cores.
- **PDF Document Loading**: `PDFLoader` with page-by-page extraction into structured `Document` instances.
- **HierarchicalChunker**: Multi-level tree generation and parent-child chunk pairings.
- **QueryAwareChunker**: Hotspot detection and adaptive chunk sizing around search queries.
- **AgenticChunker**: Discourse transition and topic segmentation.
- **SemanticChunker**: Distance threshold breakpoint clustering using sentence embeddings.
- **PropositionChunker**: Syntactic proposition decomposition into atomic factual claims.
- **ContextualChunker**: Anthropic-style situational document preface injection.

### Fixed
- Multi-byte UTF-8 boundary safety across all sliding window chunkers.
- Strict abbreviation and decimal guards in sentence splitting.

---

## [1.0.1] - 2026-09-02

### Added
- PyPI release configuration under package name `chunkr-rs`.
- Multi-platform GitHub Actions CI matrix (Linux x86_64/aarch64/armv7, Windows x64/x86, macOS x86_64/arm64).

---

## [1.0.0] - 2026-09-02

### Added
- Initial production release with core chunking strategies (`RecursiveChunker`, `TokenChunker`, `SentenceChunker`, `ParagraphChunker`, `MarkdownChunker`, `CodeChunker`, `JsonChunker`, `HtmlChunker`, `CharacterChunker`, `WordChunker`).
- PyO3 native Python extension bindings with `uv` and `maturin` build system support.
- Crates.io publication under `chunkr`.
