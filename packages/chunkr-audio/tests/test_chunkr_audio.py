"""chunkr-audio: entry-point registration and the transcriber contract.

No model is downloaded: the tests exercise registration, the lazy import path and
the `{start, end, text}` contract with a stand-in, which is what chunkr consumes.
"""

import subprocess
import sys
import wave

import pytest


def write_wav(path, seconds=2.0, rate=16000):
    """Two seconds: half silence, half tone, so the audio VAD finds speech."""
    with wave.open(str(path), "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(rate)
        frames = bytearray()
        for index in range(int(seconds * rate)):
            loud = 0.5 <= index / rate < seconds
            value = 0.3 * __import__("math").sin(2 * 3.141592653589793 * 220 * index / rate)
            frames += int(value * 32767 if loud else 0).to_bytes(2, "little", signed=True)
        handle.writeframes(bytes(frames))


def test_entry_point_registers_whisper():
    import chunkr

    assert "whisper" in chunkr.transcribers()


def test_registration_does_not_import_faster_whisper():
    """Importing chunkr must not pay for the model runtime."""
    code = (
        "import chunkr, sys;"
        "assert 'whisper' in chunkr.transcribers();"
        "print('faster_whisper' in sys.modules)"
    )
    out = subprocess.run([sys.executable, "-c", code], capture_output=True, text=True)
    assert out.returncode == 0, out.stderr
    assert out.stdout.strip() == "False", "faster-whisper was imported eagerly"


def test_audio_loads_through_the_registered_transcriber(tmp_path, monkeypatch):
    import chunkr

    calls = []

    def fake_transcribe(name):
        def transcribe(audio: bytes, audio_format: str):
            calls.append((len(audio), audio_format))
            return [
                {"start": 0.5, "end": 1.1, "text": "First thing said."},
                {"start": 1.1, "end": 1.5, "text": "Second thing said."},
                {"start": 1.5, "end": 1.6, "text": "   "},
            ]

        return transcribe

    chunkr.register_transcriber("test-audio", fake_transcribe("test-audio"))
    try:
        path = tmp_path / "clip.wav"
        write_wav(path)

        documents = chunkr.load_documents(str(path))
        assert len(documents) == 2, "blank segment dropped"
        assert documents[0].content == "First thing said."
        assert documents[0].metadata["format"] == "audio"
        assert documents[0].metadata["transcriber"] == "test-audio"
        assert documents[0].metadata["start_time_s"] == pytest.approx(0.5)
        assert documents[0].metadata["end_time_s"] == pytest.approx(1.1)
        assert documents[0].metadata["total_segments"] == 2
        assert 0.0 < documents[0].metadata["speech_ratio"] <= 1.0
        assert documents[0].metadata["sample_rate"] == 16000
        assert calls and calls[0][1] == "wav"

        # Segments merge into time windows, keeping the spans.
        chunks = chunkr.SubtitleChunker(max_characters=500).chunk_documents(documents)
        assert len(chunks) == 1
        assert "First thing said." in chunks[0].content
        assert chunks[0].metadata["cue_count"] == 2
        assert chunks[0].metadata["transcriber"] == "test-audio"
    finally:
        chunkr.unregister_transcriber("test-audio")


def test_word_timings_are_kept_as_flat_metadata(tmp_path):
    """Words are what faster-whisper/whisper.cpp return; they must survive the bridge."""
    import chunkr

    def transcribe(audio: bytes, audio_format: str):
        return [
            {
                "start": 0.5,
                "end": 1.5,
                "text": "First thing said.",
                "words": [
                    {"word": " First", "start": 0.5, "end": 0.8},
                    {"word": " thing", "start": 0.8, "end": 1.1},
                    # An SDK's own word class, not a dict.
                    type("W", (), {"word": " said.", "start": 1.1, "end": 1.5})(),
                ],
            }
        ]

    chunkr.register_transcriber("test-words", transcribe)
    try:
        path = tmp_path / "clip.wav"
        write_wav(path)
        document = chunkr.load_documents(str(path))[0]
        assert document.metadata["word_count"] == 3
        assert document.metadata["first_word_s"] == pytest.approx(0.5)
        assert document.metadata["last_word_s"] == pytest.approx(1.5)
    finally:
        chunkr.unregister_transcriber("test-words")


def test_long_audio_is_windowed_at_quiet_seams_when_asked(tmp_path, monkeypatch):
    """CHUNKR_AUDIO_WINDOW_S cuts before the model sees it; spans stay on the timeline."""
    import chunkr

    calls = []

    def transcribe(audio: bytes, audio_format: str):
        calls.append(len(audio))
        return [{"start": 0.0, "end": 0.4, "text": "piece"}]

    chunkr.register_transcriber("test-window", transcribe)
    monkeypatch.setenv("CHUNKR_AUDIO_WINDOW_S", "1")
    try:
        path = tmp_path / "long.wav"
        write_wav(path, seconds=4.0)
        documents = chunkr.load_documents(str(path))
        assert len(calls) >= 3, f"expected several calls, got {len(calls)}"
        assert all(size < 40_000 for size in calls), "each call carries one window"
        assert documents[0].metadata["audio_windows"] == len(calls)
        starts = [document.metadata["start_time_s"] for document in documents]
        assert starts == sorted(starts)
        assert starts[-1] > 1.0
    finally:
        chunkr.unregister_transcriber("test-window")


def test_whisper_transcriber_fails_with_a_clear_message_without_the_runtime():
    """The lazy import turns a missing dependency into advice, not a traceback."""
    from chunkr_audio import WhisperTranscriber

    transcriber = WhisperTranscriber()
    try:
        import faster_whisper  # noqa: F401
    except ImportError:
        with pytest.raises(RuntimeError, match="faster-whisper"):
            transcriber(b"", "wav")
    else:  # pragma: no cover - only when the runtime is installed
        pytest.skip("faster-whisper installed")
