"""Whisper speech-to-text for chunkr.

Installing this package is enough: ``import chunkr`` discovers the
``chunkr.transcribers`` entry point and registers :class:`WhisperTranscriber`, so
``chunkr.load_documents("interview.mp3")`` returns timestamped transcript segments
that chunk by time window, with per-word timings when the model provides them.

The model is created on first use, never at import: loading chunkr must not cost
a multi-hundred-megabyte download.
"""

from __future__ import annotations

import os
import tempfile
from typing import Any, Dict, Iterable, List, Optional

__version__ = "0.1.0"

__all__ = ["WhisperTranscriber", "register", "unregister"]

DEFAULT_MODEL = os.environ.get("CHUNKR_WHISPER_MODEL", "base")


class WhisperTranscriber:
    """faster-whisper backend: one model per instance, loaded on first use.

    ``model_size`` accepts any faster-whisper name (``tiny``, ``base``,
    ``small``, ``medium``, ``large-v3``, a local directory, …). ``device`` and
    ``compute_type`` are passed straight through, e.g. ``device="cuda"`` with
    ``compute_type="float16"``.
    """

    def __init__(
        self,
        model_size: str = DEFAULT_MODEL,
        device: str = "auto",
        compute_type: str = "default",
        language: Optional[str] = None,
        beam_size: int = 5,
        vad_filter: bool = False,
        word_timestamps: bool = True,
    ) -> None:
        self.model_size = model_size
        self.device = device
        self.compute_type = compute_type
        self.language = language
        self.beam_size = beam_size
        # Chunkr already measures speech; a second VAD pass is opt-in.
        self.vad_filter = vad_filter
        # Word timings let a chunk cite an exact word and be re-cut later; they
        # cost one alignment pass and are on by default.
        self.word_timestamps = word_timestamps
        self._model: Any = None

    def _load(self) -> Any:
        if self._model is None:
            try:
                from faster_whisper import WhisperModel
            except ImportError as error:  # pragma: no cover - depends on install
                raise RuntimeError(
                    "chunkr-audio needs faster-whisper: pip install faster-whisper"
                ) from error
            self._model = WhisperModel(
                self.model_size, device=self.device, compute_type=self.compute_type
            )
        return self._model

    def __call__(self, audio: bytes, audio_format: str = "wav") -> List[Dict[str, Any]]:
        """Transcribe encoded ``audio`` into ``{start, end, text, words?}`` dicts.

        faster-whisper decodes the container itself, so the bytes are written to a
        temporary file with the right extension; that keeps the ``av``/PyAV backend
        happy without chunkr having to shell out or re-encode.
        """
        model = self._load()
        suffix = f".{audio_format.lstrip('.') or 'wav'}"
        handle = tempfile.NamedTemporaryFile(suffix=suffix, delete=False)
        try:
            handle.write(audio)
            handle.flush()
            handle.close()
            segments, _info = model.transcribe(
                handle.name,
                language=self.language,
                beam_size=self.beam_size,
                vad_filter=self.vad_filter,
                word_timestamps=self.word_timestamps,
            )
            return [
                segment_document(segment)
                for segment in segments
                if segment.text.strip()
            ]
        finally:
            os.unlink(handle.name)

    def __repr__(self) -> str:  # pragma: no cover - debugging aid
        return f"WhisperTranscriber(model_size={self.model_size!r}, device={self.device!r})"


def segment_document(segment: Any) -> Dict[str, Any]:
    """One faster-whisper segment as a chunkr transcriber returns it.

    ``words`` is passed through whenever the model produced it — chunkr turns it
    into ``word_count``/``first_word_s``/``last_word_s`` plus the word list a
    caller needs to cite or re-cut.
    """
    document: Dict[str, Any] = {
        "start": float(segment.start),
        "end": float(segment.end),
        "text": segment.text.strip(),
    }
    words = getattr(segment, "words", None) or []
    timings = [
        {"word": word.word, "start": float(word.start), "end": float(word.end)}
        for word in words
        if getattr(word, "word", "").strip()
    ]
    if timings:
        document["words"] = timings
    return document


def register() -> str:
    """Register the Whisper transcriber with chunkr; returns its name.

    Called by the ``chunkr.transcribers`` entry point on ``import chunkr``.
    """
    import chunkr

    chunkr.register_transcriber("whisper", WhisperTranscriber())
    return "whisper"


def unregister() -> bool:
    """Remove the Whisper transcriber again."""
    import chunkr

    return chunkr.unregister_transcriber("whisper")
