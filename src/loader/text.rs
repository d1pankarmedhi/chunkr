//! Text-format extractors: encoding detection, RTF, XML and Jupyter notebooks.

use serde_json::Value;

use crate::error::ChunkrError;

/// Decode raw bytes into UTF-8, honouring byte-order marks and falling back to
/// Windows-1252 for legacy single-byte encodings.
pub fn decode_text(bytes: &[u8]) -> String {
    if let Some((encoding, bom_len)) = encoding_rs::Encoding::for_bom(bytes) {
        return encoding.decode(&bytes[bom_len..]).0.into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        // Windows-1252 maps every byte value, so this never loses content.
        Err(_) => encoding_rs::WINDOWS_1252
            .decode_without_bom_handling(bytes)
            .0
            .into_owned(),
    }
}

/// True when the payload looks binary rather than text.
pub fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8192).any(|b| *b == 0)
}

/// Normalise extracted text: trim lines, collapse inner runs of whitespace and
/// keep at most one blank line between paragraphs.
pub fn clean_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank_pending = false;
    for line in text.lines() {
        let trimmed = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if trimmed.is_empty() {
            blank_pending = !out.is_empty();
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
            if blank_pending {
                out.push('\n');
            }
        }
        blank_pending = false;
        out.push_str(&trimmed);
    }
    out
}

/// Destinations whose contents are formatting metadata rather than document text.
const RTF_SKIP: &[&str] = &[
    "fonttbl",
    "colortbl",
    "stylesheet",
    "info",
    "pict",
    "object",
    "themedata",
    "datastore",
    "latentstyles",
    "listtable",
    "listoverridetable",
    "rsidtbl",
    "filetbl",
    "revtbl",
    "generator",
];

/// Extract readable text from an RTF document.
pub fn rtf_to_text(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len() / 4);
    let mut groups: Vec<bool> = Vec::new();
    let mut skipping = false;
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                groups.push(skipping);
                i += 1;
            }
            b'}' => {
                skipping = groups.pop().unwrap_or(false);
                i += 1;
            }
            b'\\' => {
                i += 1;
                if i >= bytes.len() {
                    break;
                }
                match bytes[i] {
                    b'\\' | b'{' | b'}' => {
                        if !skipping {
                            out.push(bytes[i] as char);
                        }
                        i += 1;
                    }
                    b'\'' => {
                        // Two-digit hex escape in the document code page.
                        if let Some(byte) = input
                            .get(i + 1..i + 3)
                            .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                        {
                            if !skipping {
                                out.push_str(&decode_text(&[byte]));
                            }
                        }
                        i += 3;
                    }
                    b'*' => {
                        // Ignorable destination: drop the whole group.
                        skipping = true;
                        i += 1;
                    }
                    b'\n' | b'\r' => i += 1,
                    _ => {
                        let start = i;
                        while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                            i += 1;
                        }
                        let word = &input[start..i];
                        let number_start = i;
                        if i < bytes.len() && (bytes[i] == b'-' || bytes[i].is_ascii_digit()) {
                            i += 1;
                            while i < bytes.len() && bytes[i].is_ascii_digit() {
                                i += 1;
                            }
                        }
                        let number = input[number_start..i].parse::<i32>().ok();
                        if i < bytes.len() && bytes[i] == b' ' {
                            i += 1;
                        }

                        if RTF_SKIP.contains(&word) {
                            skipping = true;
                            continue;
                        }
                        if skipping {
                            continue;
                        }
                        match word {
                            "par" | "line" | "page" | "sect" | "row" | "cell" => out.push('\n'),
                            "tab" => out.push('\t'),
                            "u" => {
                                if let Some(code) = number {
                                    let code = if code < 0 { code + 0x10000 } else { code };
                                    if let Some(ch) = char::from_u32(code as u32) {
                                        out.push(ch);
                                    }
                                }
                                // Drop the ANSI fallback character that follows \uN.
                                if i < bytes.len() && bytes[i] != b'\\' {
                                    i += input[i..].chars().next().map_or(1, char::len_utf8);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {
                let start = i;
                while i < bytes.len() && !matches!(bytes[i], b'\\' | b'{' | b'}') {
                    i += 1;
                }
                if !skipping {
                    out.push_str(&input[start..i]);
                }
            }
        }
    }

    clean_text(&out)
}

/// Extract the concatenated character data of an XML document, one line per element.
pub fn xml_to_text(xml: &str) -> Result<String, ChunkrError> {
    use quick_xml::events::Event;

    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut out = String::with_capacity(xml.len() / 2);

    loop {
        match reader.read_event().map_err(parse_error)? {
            Event::Text(text) => out.push_str(&text.xml10_content()),
            Event::CData(data) => {
                out.push_str(&data.into_inner());
            }
            Event::End(_) => {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(clean_text(&out))
}

/// Render a Jupyter notebook as Markdown: code fenced, outputs appended, cells in order.
pub fn notebook_to_text(json: &str) -> Result<String, ChunkrError> {
    let notebook: Value = serde_json::from_str(json)
        .map_err(|e| ChunkrError::ParseError(format!("invalid notebook JSON: {e}")))?;

    let mut out = String::with_capacity(json.len() / 2);
    let cells = notebook.get("cells").and_then(Value::as_array);

    for cell in cells.into_iter().flatten() {
        let source = json_text(cell.get("source"));
        let source = source.trim_end();
        match cell.get("cell_type").and_then(Value::as_str) {
            Some("code") if !source.is_empty() => {
                out.push_str("```\n");
                out.push_str(source);
                out.push_str("\n```\n\n");
            }
            Some("markdown") | Some("raw") if !source.is_empty() => {
                out.push_str(source);
                out.push_str("\n\n");
            }
            _ => {}
        }

        for output in cell
            .get("outputs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let text = match output.get("text") {
                Some(value) => json_text(Some(value)),
                None => json_text(output.get("data").and_then(|data| data.get("text/plain"))),
            };
            let text = text.trim_end();
            if !text.is_empty() {
                out.push_str("```\n");
                out.push_str(text);
                out.push_str("\n```\n\n");
            }
        }
    }

    Ok(out.trim_end().to_string())
}

/// Notebook string fields are either a plain string or an array of lines.
fn json_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(""),
        Some(Value::String(text)) => text.clone(),
        _ => String::new(),
    }
}

pub(crate) fn parse_error<E: std::fmt::Display>(error: E) -> ChunkrError {
    ChunkrError::ParseError(error.to_string())
}
