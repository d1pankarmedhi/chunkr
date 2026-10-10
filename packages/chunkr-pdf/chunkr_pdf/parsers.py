"""Page-level parser backends for chunkr: whole-document services instead of OCR.

OCR engines (see `_ocr`) fill in the words on a page; these backends *replace*
the parse. They speak to document-parsing services that return markdown or
structured text per page — docling-serve, Mistral OCR, or any OpenAI-compatible
vision model (olmOCR, Qwen-VL, dots.ocr, or a hosted VLM) — and hand the result
back to chunkr as a page payload, so every sanitizer, granularity and metadata
rule still applies.

    from chunkr_pdf.parsers import docling, vlm, mistral

    chunkr.register_pdf_backend("docling", docling("http://localhost:5001"))
    documents = chunkr.PDFLoader(backend="docling").load("report.pdf")

Nothing here needs credentials except the services themselves, and no vendor SDK
is imported: requests go out with `urllib` and responses are normalised locally.
"""

from __future__ import annotations

import json
import re
import urllib.error
import urllib.request
from typing import Any, Callable, Dict, List, Optional

__all__ = ["docling", "vlm", "mistral", "markdown_blocks", "payload_from_pages"]


# ── markdown -> block structure ──────────────────────────────────────────

_HEADING = re.compile(r"^(#{1,6})\s+(.*)$")
_BULLET = re.compile(r"^[-*+]\s+(.*)$")
_ORDERED = re.compile(r"^(\d{1,3})[.)]\s+(.*)$")


def markdown_blocks(markdown: str) -> List[Dict[str, Any]]:
    """Split markdown into the block structure chunkr's mapper understands.

    Headings, list items and paragraphs are recognised; anything else stays a
    paragraph so no text is lost. Table syntax is left as text because chunkr's
    sanitizer decides what to do with it.
    """
    blocks: List[Dict[str, Any]] = []
    paragraph: List[str] = []

    def flush() -> None:
        if paragraph:
            text = "\n".join(paragraph).strip()
            if text:
                blocks.append({"kind": "paragraph", "text": text})
            paragraph.clear()

    for raw_line in markdown.splitlines():
        line = raw_line.rstrip()
        if not line.strip():
            flush()
            continue
        heading = _HEADING.match(line)
        if heading:
            flush()
            blocks.append(
                {
                    "kind": "heading",
                    "text": heading.group(2).strip(),
                    "level": len(heading.group(1)),
                }
            )
            continue
        bullet = _BULLET.match(line)
        if bullet:
            flush()
            blocks.append({"kind": "list_item", "text": bullet.group(1).strip(), "ordered": False})
            continue
        ordered = _ORDERED.match(line)
        if ordered:
            flush()
            blocks.append(
                {
                    "kind": "list_item",
                    "text": ordered.group(2).strip(),
                    "ordered": True,
                    "marker": ordered.group(1),
                }
            )
            continue
        paragraph.append(line)
    flush()
    return blocks


# ── payload assembly ─────────────────────────────────────────────────────


def payload_from_pages(pages: List[Dict[str, Any]], version: str) -> str:
    """Wrap page dicts into the payload chunkr's mapper expects.

    Each page takes `markdown` and/or `text`; markdown is split into blocks so
    structured output works too.
    """
    out: List[Dict[str, Any]] = []
    for index, page in enumerate(pages, start=1):
        markdown = page.get("markdown") or ""
        text = page.get("text") or ""
        entry: Dict[str, Any] = {
            "page_number": page.get("page_number") or index,
        }
        if page.get("page_label"):
            entry["page_label"] = page["page_label"]
        entry["text"] = text or _markdown_text(markdown)
        if markdown:
            entry["markdown"] = markdown
        blocks = page.get("blocks")
        entry["blocks"] = blocks if blocks is not None else markdown_blocks(markdown)
        out.append(entry)
    return json.dumps({"version": version, "pages": out}, ensure_ascii=False)


def _markdown_text(markdown: str) -> str:
    """Plain text of markdown: syntax markers removed, structure kept."""
    lines = []
    for line in markdown.splitlines():
        stripped = _HEADING.sub(r"\2", line.strip())
        stripped = _BULLET.sub(r"\1", stripped)
        stripped = _ORDERED.sub(r"\2", stripped)
        lines.append(stripped)
    return "\n".join(lines).strip()


# ── http plumbing ────────────────────────────────────────────────────────


class _ServiceError(RuntimeError):
    pass


def _post_json(url: str, payload: Dict[str, Any], headers: Dict[str, str], timeout: float) -> Dict[str, Any]:
    body = json.dumps(payload).encode()
    request = urllib.request.Request(
        url,
        data=body,
        headers={"Content-Type": "application/json", **headers},
        method="POST",
    )
    return _read_json(request, timeout, url)


def _post_form(
    url: str,
    fields: Dict[str, Any],
    files: Dict[str, tuple],
    headers: Dict[str, str],
    timeout: float,
) -> Dict[str, Any]:
    boundary = "----chunkrpdf"
    body = b""
    for name, value in fields.items():
        body += f"--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n".encode()
        body += (
            value if isinstance(value, bytes) else json.dumps(value).encode()
            if not isinstance(value, str)
            else value.encode()
        ) + b"\r\n"
    for name, (filename, data) in files.items():
        body += (
            f"--{boundary}\r\n"
            f'Content-Disposition: form-data; name="{name}"; filename="{filename}"\r\n'
            "Content-Type: application/pdf\r\n\r\n"
        ).encode()
        body += data + b"\r\n"
    body += f"--{boundary}--\r\n".encode()
    request = urllib.request.Request(
        url,
        data=body,
        headers={"Content-Type": f"multipart/form-data; boundary={boundary}", **headers},
        method="POST",
    )
    return _read_json(request, timeout, url)


def _read_json(request: urllib.request.Request, timeout: float, url: str) -> Dict[str, Any]:
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return json.loads(response.read() or b"{}")
    except urllib.error.HTTPError as error:  # surface the service's message
        detail = error.read().decode(errors="replace")[:500]
        raise _ServiceError(f"{url} returned HTTP {error.code}: {detail}") from None
    except urllib.error.URLError as error:
        raise _ServiceError(f"{url} is unreachable: {error.reason}") from None


def _source_bytes(source: Any) -> bytes:
    if isinstance(source, (bytes, bytearray)):
        return bytes(source)
    with open(str(source), "rb") as handle:
        return handle.read()


# ── backends ─────────────────────────────────────────────────────────────


def docling(url: str, *, options: Optional[Dict[str, Any]] = None, timeout: float = 600.0) -> Callable:
    """docling-serve `POST /v1/convert/file` as a chunkr parser backend.

    Prefers the structured `json_content` (grouped by `prov.page_no`, labels
    mapped to block kinds) and falls back to `md_content` as a single page.
    """
    endpoint = f"{url.rstrip('/')}/v1/convert/file"
    request_options = dict(options or {})

    def backend(source: Any, config_json: str = "") -> str:
        config = json.loads(config_json) if config_json else {}
        fields: Dict[str, Any] = {"to_formats": ["md", "json"]}
        fields.update(request_options)
        response = _post_form(
            endpoint,
            fields,
            {"files": ("document.pdf", _source_bytes(source))},
            {},
            timeout,
        )
        document = response.get("document") or {}
        structured = document.get("json_content") or document.get("json")
        if structured and structured.get("texts"):
            return payload_from_pages(_pages_from_docling(structured), "docling")
        markdown = document.get("md_content") or document.get("md") or ""
        if not markdown:
            raise _ServiceError(f"{endpoint} returned no content: {list(response)}")
        return payload_from_pages([{"markdown": markdown}], "docling")

    return backend


_DOCLING_LABELS = {
    "section_header": "heading",
    "title": "heading",
    "list_item": "list_item",
    "caption": "paragraph",
    "table": "table",
    "code": "code",
    "reference": "paragraph",
}


def _pages_from_docling(structured: Dict[str, Any]) -> List[Dict[str, Any]]:
    """Group docling text items by page, keeping their labels and levels."""
    pages: Dict[int, List[Dict[str, Any]]] = {}
    for item in structured.get("texts") or []:
        text = (item.get("text") or item.get("orig") or "").strip()
        if not text:
            continue
        page_number = 1
        for provenance in item.get("prov") or []:
            if provenance.get("page_no"):
                page_number = int(provenance["page_no"])
                break
        label = str(item.get("label") or "text")
        block: Dict[str, Any] = {
            "kind": _DOCLING_LABELS.get(label, "paragraph"),
            "text": text,
        }
        if block["kind"] == "heading":
            block["level"] = int(item.get("level") or 1)
        pages.setdefault(page_number, []).append(block)
    return [
        {"page_number": number, "blocks": blocks, "text": "\n\n".join(b["text"] for b in blocks)}
        for number, blocks in sorted(pages.items())
    ]


def mistral(
    api_key: str,
    *,
    model: str = "mistral-ocr-latest",
    base_url: str = "https://api.mistral.ai",
    timeout: float = 600.0,
    include_images: bool = False,
) -> Callable:
    """Mistral OCR (`POST /v1/ocr`) as a chunkr parser backend.

    Mistral returns one markdown (and optional bbox) block per page; page
    markdown is kept verbatim (`sanitize={"enabled": False}` gives raw
    passthrough) and also split into blocks for structured output.
    """
    import base64

    endpoint = f"{base_url.rstrip('/')}/v1/ocr"
    headers = {"Authorization": f"Bearer {api_key}"}

    def backend(source: Any, config_json: str = "") -> str:
        data = base64.b64encode(_source_bytes(source)).decode()
        response = _post_json(
            endpoint,
            {
                "model": model,
                "document": {
                    "type": "document_url",
                    "document_url": f"data:application/pdf;base64,{data}",
                },
                "include_image_base64": bool(include_images),
            },
            headers,
            timeout,
        )
        pages = response.get("pages") or []
        if not pages:
            raise _ServiceError(f"{endpoint} returned no pages: {list(response)}")
        return payload_from_pages(
            [
                {
                    "page_number": int(page.get("index", index)) + 1,
                    "markdown": page.get("markdown") or "",
                }
                for index, page in enumerate(pages)
            ],
            f"mistral-ocr:{model}",
        )

    return backend


def vlm(
    base_url: str,
    model: str,
    *,
    api_key: Optional[str] = None,
    prompt: Optional[str] = None,
    dpi: float = 200.0,
    timeout: float = 600.0,
    max_tokens: int = 8192,
    render: Optional[Callable[[bytes, float], List[bytes]]] = None,
) -> Callable:
    """Any OpenAI-compatible vision endpoint as a page-level OCR backend.

    Each page is rasterised with liteparse's screenshot renderer at `dpi` and
    sent as a data URL to `POST {base_url}/chat/completions`; the reply is used
    as that page's markdown. This is the route for olmOCR, Qwen-VL, dots.ocr,
    Gemini/OpenAI vision and any self-hosted VLM (vLLM, LM Studio, ...).

    `render` overrides rasterisation (used by tests); it receives the PDF bytes
    and DPI and returns one PNG per page.
    """
    endpoint = f"{base_url.rstrip('/')}/chat/completions"
    headers = {"Authorization": f"Bearer {api_key}"} if api_key else {}
    instruction = prompt or (
        "Transcribe this page to markdown. Preserve reading order, headings and "
        "table structure. Output only the markdown."
    )
    renderer = render or _render_pages

    def backend(source: Any, config_json: str = "") -> str:
        data = _source_bytes(source)
        pages = renderer(data, dpi)
        rendered: List[Dict[str, Any]] = []
        for index, png in enumerate(pages, start=1):
            import base64

            image = base64.b64encode(png).decode()
            response = _post_json(
                endpoint,
                {
                    "model": model,
                    "max_tokens": max_tokens,
                    "temperature": 0,
                    "messages": [
                        {
                            "role": "user",
                            "content": [
                                {"type": "text", "text": instruction},
                                {
                                    "type": "image_url",
                                    "image_url": {"url": f"data:image/png;base64,{image}"},
                                },
                            ],
                        }
                    ],
                },
                headers,
                timeout,
            )
            markdown = _completion_text(response, endpoint)
            if markdown:
                rendered.append({"page_number": index, "markdown": markdown})
        if not rendered:
            raise _ServiceError(f"{endpoint} returned no page text")
        return payload_from_pages(rendered, f"vlm:{model}")

    return backend


def _completion_text(response: Dict[str, Any], endpoint: str) -> str:
    choices = response.get("choices") or []
    if not choices:
        raise _ServiceError(f"{endpoint} returned no choices: {list(response)}")
    message = choices[0].get("message") or {}
    content = message.get("content")
    if isinstance(content, list):  # some servers return content parts
        content = "".join(part.get("text", "") for part in content if isinstance(part, dict))
    return (content or "").strip()


def _render_pages(data: bytes, dpi: float) -> List[bytes]:
    """Rasterise a PDF with liteparse's screenshot renderer."""
    import liteparse

    parser = liteparse.LiteParse(
        extract_screenshots=True, dpi=dpi, ocr_enabled=False, quiet=True
    )
    result = parser.parse(data)
    return [shot.image_bytes for shot in result.screenshots]
