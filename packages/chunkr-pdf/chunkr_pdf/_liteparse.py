"""liteparse adapter: extraction only, mapping stays in chunkr core."""

from __future__ import annotations

import dataclasses
import inspect
import json
from typing import Any, Dict, Iterator, List, Optional, Tuple, Union

PathLike = Union[str, bytes]

BACKEND_NAME = "liteparse"


def liteparse_version() -> str:
    import liteparse

    return getattr(liteparse, "__version__", "unknown")


def _prune(value: Any) -> Any:
    """Drop None and empty containers so payloads stay small."""
    if isinstance(value, dict):
        return {k: _prune(v) for k, v in value.items() if v not in (None, [], "", {})}
    if isinstance(value, list):
        return [_prune(v) for v in value]
    return value


def _as_dict(value: Any) -> Any:
    return _prune(dataclasses.asdict(value)) if dataclasses.is_dataclass(value) else None


def liteparse_kwargs(
    config: Dict[str, Any], overrides: Optional[Dict[str, Any]] = None
) -> Dict[str, Any]:
    """Translate a chunkr parser config into liteparse constructor kwargs.

    Mirrors `chunkr::parser::liteparse_backend::liteparse_config`; options the
    installed liteparse version does not accept are dropped.
    """
    extract = config.get("extract") or {}
    sanitize = config.get("sanitize") or {}
    ocr = config.get("ocr") or {}
    scope = config.get("scope") or {}
    headers = ocr.get("headers") or []

    kwargs: Dict[str, Any] = {
        "ocr_enabled": ocr.get("mode", "off") in ("auto", "always", "server"),
        "ocr_language": ocr.get("language", "eng"),
        "ocr_server_url": ocr.get("server_url"),
        "ocr_server_headers": dict(headers) if headers else None,
        "ocr_hedge_delays_ms": list(ocr.get("hedge_delays_ms") or []) or None,
        "tessdata_path": ocr.get("tessdata_path"),
        "ocr_failure_fatal": bool(ocr.get("failure_fatal", False)),
        "num_workers": ocr.get("num_workers") or None,
        "max_pages": scope.get("max_pages"),
        "target_pages": scope.get("target_pages"),
        "password": scope.get("password"),
        "dpi": scope.get("dpi"),
        "preserve_very_small_text": bool(scope.get("preserve_very_small_text", False)),
        "skip_diagonal_text": bool(scope.get("skip_diagonal_text", False)),
        "keep_headers_footers": bool(scope.get("keep_headers_footers", False)),
        "quiet": bool(scope.get("quiet", True)),
        # Markdown passthrough needs the backend renderer; sanitized markdown is
        # rendered by chunkr from the blocks instead.
        "output_format": "markdown"
        if config.get("output") in ("markdown", "both") and not sanitize.get("enabled", True)
        else "text",
        "extract_blocks": bool(extract.get("blocks", True)),
        "include_complexity": bool(config.get("include_complexity", False)),
        "extract_links": bool(extract.get("links", True)),
        "image_mode": "off" if extract.get("figures") == "skip" else "placeholder",
        "extract_images": bool(extract.get("images", False)),
        "extract_form_fields": bool(extract.get("form_fields", False)),
        "extract_annotations": bool(extract.get("annotations", False)),
        "extract_structure_tree": bool(extract.get("structure_tree", False)),
        "extract_vector_graphics": bool(extract.get("vector_graphics", False)),
        "extract_text_metadata": bool(extract.get("text_metadata", False)),
        "extract_screenshots": bool(extract.get("screenshots", False)),
        "continue_on_page_error": config.get("on_error") == "page_error",
    }
    # Per-parse overrides: page selection for OCR rounds, loopback server URL,
    # forced language. They win over whatever the config says.
    kwargs.update(overrides or {})
    return _supported_kwargs(kwargs)


def _supported_kwargs(kwargs: Dict[str, Any]) -> Dict[str, Any]:
    """Keep only options this liteparse build understands."""
    from liteparse import LiteParse

    try:
        accepted = set(inspect.signature(LiteParse.__init__).parameters) - {"self"}
    except (TypeError, ValueError):  # pragma: no cover - builtins without signature
        return kwargs
    if not accepted:
        return kwargs
    return {key: value for key, value in kwargs.items() if key in accepted}


def build_parser(config: Dict[str, Any], overrides: Optional[Dict[str, Any]] = None):
    """Instantiate a liteparse parser for a chunkr parser config."""
    from liteparse import LiteParse

    return LiteParse(**liteparse_kwargs(config, overrides))


def page_payload(page: Any) -> Dict[str, Any]:
    return _prune(
        {
            "page_number": page.page_num,
            "page_label": page.page_label,
            "page_width": page.width,
            "page_height": page.height,
            "text": page.text,
            "markdown": page.markdown or None,
            "blocks": [_as_dict(block) for block in (page.blocks or [])],
            "complexity": _as_dict(page.complexity),
        }
    )


def result_payload(result: Any) -> Dict[str, Any]:
    return {
        "version": liteparse_version(),
        "pages": [page_payload(page) for page in result.pages],
    }


def parse_payload(source: Any, config_json: str) -> str:
    """Backend callable registered with chunkr: extract, do not map."""
    config = json.loads(config_json) if config_json else {}
    return payload(source, config)


def payload(source: Any, config: Dict[str, Any], overrides: Optional[Dict[str, Any]] = None) -> str:
    """Parse `source` and return the payload JSON, with optional overrides."""
    parser = build_parser(config, overrides)
    result = parser.parse(source if isinstance(source, bytes) else str(source))
    return json.dumps(result_payload(result), ensure_ascii=False)


def parse_batches(
    source: Any,
    config: Dict[str, Any],
    batch_size: int,
    overrides: Optional[Dict[str, Any]] = None,
) -> Iterator[str]:
    """Yield one payload JSON per page batch (bounded memory)."""
    parser = build_parser(config, overrides)
    for batch in parser.parse_batches(source, batch_size):
        payload = result_payload(batch.result)
        payload["start_page"] = batch.start_page
        payload["end_page"] = batch.end_page
        yield json.dumps(payload, ensure_ascii=False)


def complexity_rows(source: Any, config: Optional[Dict[str, Any]] = None) -> List[Dict[str, Any]]:
    """Per-page complexity signals without a full parse."""
    parser = build_parser(config or {"ocr": {"mode": "off"}})
    return [_as_dict(row) for row in parser.is_complex(source)]


def register() -> None:
    """Entry point: make this backend discoverable by chunkr."""
    import chunkr

    chunkr.register_pdf_backend(BACKEND_NAME, parse_payload)
