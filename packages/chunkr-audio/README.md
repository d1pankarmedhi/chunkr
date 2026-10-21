# chunkr-audio

Speech-to-text for [`chunkr`](https://github.com/d1pankarmedhi/chunkr): Whisper
transcripts with timestamps, so audio chunks the way subtitles do — one
timestamped unit per segment, merged into time windows that never cut a sentence
in half.

```bash
pip install chunkr-rs[audio] chunkr-audio
```

```python
import chunkr

documents = chunkr.load_documents("interview.mp3")   # one Document per segment
chunks = chunkr.SubtitleChunker(max_seconds=60).chunk_documents(documents)

for chunk in chunks:
    print(chunk.metadata["start_time_s"], chunk.metadata["end_time_s"], chunk.content[:60])
```

`import chunkr` discovers this package through the `chunkr.transcribers` entry
point, so no explicit registration is needed. The model is built on first
transcription and cached on the instance; importing `chunkr` never downloads it.

## Configuration

```python
import chunkr
from chunkr_audio import WhisperTranscriber

chunkr.register_transcriber("small-fr", WhisperTranscriber(model_size="small", language="fr"))
```

Then pick it either by name for one call or process-wide:

```bash
CHUNKR_TRANSCRIBER=small-fr python app.py
```

| Option | Default | Why |
| :--- | :--- | :--- |
| `model_size` | `CHUNKR_WHISPER_MODEL` or `base` | `tiny` … `large-v3`, or a local directory |
| `device` | `auto` | `cpu`, `cuda`, `auto` |
| `compute_type` | `default` | `int8`, `float16`, … |
| `language` | `None` (detect) | Pinning it is faster and steadier on short clips |
| `beam_size` | `5` | Lower is faster, rougher |
| `vad_filter` | `False` | chunkr already measures speech; enable to also let faster-whisper drop silence |

## What chunkr adds

The core decodes the container (pure Rust, no system codecs), measures the audio,
and records provenance on every segment: `format="audio"`, `audio_format`,
`audio_duration_s`, `sample_rate`, `channels`, `speech_s`, `speech_ratio`,
`transcriber`, `segment_index`, `total_segments`, `start_time_s`, `end_time_s`.

Anything else that produces `{start, end, text, speaker?}` works too — a hosted
API, an in-house model, diarization output — through
`chunkr.register_transcriber(name, fn)`, no Rust changes needed.
