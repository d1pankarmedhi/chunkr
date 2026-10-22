//! Cell number formats, recovered from the xlsx parts calamine does not expose.
//!
//! calamine turns date and duration formats into typed cells but drops the rest
//! of the style table, so a percentage arrives as `0.15` and currency as
//! `1234.5`. The display format lives in `xl/styles.xml` (`numFmts` +
//! `cellXfs`) and in each cell's `s=` attribute, so this reads those two parts
//! and renders numbers the way the spreadsheet shows them.
//!
//! Only xlsx is covered: xls/xlsb/ods keep their stored values, since the
//! extraction path for those formats does not expose a style index.

use std::collections::HashMap;
use std::io::{Cursor, Read};

/// Display formats for one worksheet, keyed by absolute cell position.
#[derive(Debug, Default, Clone)]
pub(crate) struct SheetFormats {
    formats: HashMap<(u32, u32), String>,
}

impl SheetFormats {
    /// Read the number formats of `sheet_name`, or `None` when the workbook is
    /// not xlsx or exposes no styles.
    pub(crate) fn read(bytes: &[u8], sheet_name: &str) -> Option<Self> {
        let mut archive = open(bytes)?;
        let codes = format_codes(&mut archive)?;
        let part = sheet_part(&mut archive, sheet_name)?;
        let xml = read_part(&mut archive, &part)?;
        let formats = cell_formats(&xml, &codes);

        if formats.is_empty() {
            None
        } else {
            Some(Self { formats })
        }
    }

    /// Render a numeric cell with its display format, when it has one.
    pub(crate) fn render(&self, row: u32, column: u32, value: f64) -> Option<String> {
        let code = self.formats.get(&(row, column))?;
        render_number(value, code)
    }
}

fn open(bytes: &[u8]) -> Option<zip::ZipArchive<Cursor<Vec<u8>>>> {
    zip::ZipArchive::new(Cursor::new(bytes.to_vec())).ok()
}

fn read_part(archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>, name: &str) -> Option<String> {
    let mut file = archive.by_name(name).ok()?;
    let mut buffer = String::new();
    file.read_to_string(&mut buffer).ok()?;
    Some(buffer)
}

/// `cellXfs` index (the style a cell's `s=` names) -> format code.
///
/// Only `cellXfs` is indexed: the `xf` entries in `cellStyleXfs` are named-cell
/// styles and are not addressable from `s=`. Style index 0 is `General`, which
/// stays unrendered rather than memorised.
fn format_codes(archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>) -> Option<HashMap<u16, String>> {
    let xml = read_part(archive, "xl/styles.xml")?;
    let mut custom: HashMap<u16, String> = HashMap::new();
    let mut styles: Vec<u16> = Vec::new();
    let mut in_cell_xfs = false;

    let mut reader = quick_xml::Reader::from_str(&xml);
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(event))
            | Ok(quick_xml::events::Event::Empty(event)) => match event.name().as_ref() {
                "numFmt" => {
                    let mut format_id = None;
                    let mut code = None;
                    for attribute in event.attributes().flatten() {
                        let value = attribute_value(&attribute);
                        match attribute.key.as_ref() {
                            "numFmtId" => format_id = value.parse::<u16>().ok(),
                            "formatCode" => code = Some(value),
                            _ => {}
                        }
                    }
                    if let (Some(format_id), Some(code)) = (format_id, code) {
                        custom.insert(format_id, code);
                    }
                }
                "cellXfs" => in_cell_xfs = true,
                "xf" if in_cell_xfs => {
                    styles.push(
                        event
                            .attributes()
                            .flatten()
                            .find(|attribute| attribute.key.as_ref() == "numFmtId")
                            .and_then(|attribute| attribute_value(&attribute).parse().ok())
                            .unwrap_or(0),
                    );
                }
                _ => {}
            },
            Ok(quick_xml::events::Event::End(event)) if event.name().as_ref() == "cellXfs" => {
                in_cell_xfs = false;
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    let builtin = builtin_codes();
    let mut resolved = HashMap::new();
    for (index, format_id) in styles.iter().enumerate() {
        let code = custom.get(format_id).or_else(|| builtin.get(format_id));
        if let Some(code) = code {
            resolved.insert(index as u16, code.clone());
        }
    }
    Some(resolved)
}

/// The parts of the style table we can render faithfully.
///
/// Currency builtins are locale-resolved by Excel; `$` is used for them, which
/// matches the default en-US rendering.
fn builtin_codes() -> HashMap<u16, String> {
    let codes: &[(u16, &str)] = &[
        (1, "0"),
        (2, "0.00"),
        (3, "#,##0"),
        (4, "#,##0.00"),
        (5, "$#,##0_);($#,##0)"),
        (6, "$#,##0_);[Red]($#,##0)"),
        (7, "$#,##0.00_);($#,##0.00)"),
        (8, "$#,##0.00_);[Red]($#,##0.00)"),
        (9, "0%"),
        (10, "0.00%"),
        (37, "#,##0_);(#,##0)"),
        (38, "#,##0_);[Red](#,##0)"),
        (39, "#,##0.00_);(#,##0.00)"),
        (40, "#,##0.00_);[Red](#,##0.00)"),
    ];
    codes
        .iter()
        .map(|(id, code)| (*id, code.to_string()))
        .collect()
}

/// Worksheet part path for `sheet_name`, via `workbook.xml` and its rels.
fn sheet_part(archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>, sheet_name: &str) -> Option<String> {
    let workbook = read_part(archive, "xl/workbook.xml")?;
    let mut relation_id = None;
    let mut reader = quick_xml::Reader::from_str(&workbook);
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(event))
            | Ok(quick_xml::events::Event::Empty(event))
                if event.name().as_ref() == "sheet" =>
            {
                let mut name = None;
                let mut id = None;
                for attribute in event.attributes().flatten() {
                    let value = attribute_value(&attribute);
                    match attribute.key.as_ref() {
                        "name" => name = Some(value),
                        "r:id" => id = Some(value),
                        _ => {}
                    }
                }
                if name.as_deref() == Some(sheet_name) {
                    relation_id = id;
                    break;
                }
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    let relation_id = relation_id?;
    let rels = read_part(archive, "xl/_rels/workbook.xml.rels")?;
    let mut reader = quick_xml::Reader::from_str(&rels);
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(event))
            | Ok(quick_xml::events::Event::Empty(event))
                if event.name().as_ref() == "Relationship" =>
            {
                let mut id = None;
                let mut target = None;
                for attribute in event.attributes().flatten() {
                    let value = attribute_value(&attribute);
                    match attribute.key.as_ref() {
                        "Id" => id = Some(value),
                        "Target" => target = Some(value),
                        _ => {}
                    }
                }
                if id.as_deref() == Some(relation_id.as_str()) {
                    let target = target?;
                    return Some(if target.starts_with('/') {
                        target.trim_start_matches('/').to_string()
                    } else {
                        format!("xl/{target}")
                    });
                }
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    None
}

/// Style index per cell, resolved to format codes through `cellXfs`.
fn cell_formats(xml: &str, styles: &HashMap<u16, String>) -> HashMap<(u32, u32), String> {
    let mut formats = HashMap::new();
    let mut reader = quick_xml::Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(event))
            | Ok(quick_xml::events::Event::Empty(event))
                if event.name().as_ref() == "c" =>
            {
                let mut reference = None;
                let mut style = None;
                for attribute in event.attributes().flatten() {
                    let value = attribute_value(&attribute);
                    match attribute.key.as_ref() {
                        "r" => reference = Some(value),
                        "s" => style = value.parse::<u16>().ok(),
                        _ => {}
                    }
                }
                if let (Some(reference), Some(style)) = (reference, style) {
                    if let (Some(position), Some(code)) =
                        (cell_position(&reference), styles.get(&style))
                    {
                        formats.insert(position, code.clone());
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    formats
}

/// `B3` -> (2, 1), zero-based.
fn cell_position(reference: &str) -> Option<(u32, u32)> {
    let mut row = 0u32;
    let mut column = 0u32;
    let mut in_row = false;

    for character in reference.chars() {
        match character {
            'A'..='Z' | 'a'..='z' if !in_row => {
                column = column
                    .checked_mul(26)?
                    .checked_add(character.to_ascii_uppercase() as u32 - 'A' as u32 + 1)?;
            }
            '0'..='9' => {
                in_row = true;
                row = row
                    .checked_mul(10)?
                    .checked_add(character as u32 - '0' as u32)?;
            }
            '$' => {}
            _ => return None,
        }
    }

    if row == 0 || column == 0 {
        return None;
    }
    Some((row - 1, column - 1))
}

/// Render `value` the way `code` (an Excel number format) displays it.
///
/// `None` means "no display format worth applying" — either the code carries no
/// numeric skeleton (`General`, text, dates, which calamine already typed) or it
/// uses syntax this renderer does not implement.
pub(crate) fn render_number(value: f64, code: &str) -> Option<String> {
    let sections = split_sections(code);
    if sections.is_empty() {
        return None;
    }

    let (section, magnitude) = if value < 0.0 && sections.len() >= 2 {
        (sections[1].as_str(), -value)
    } else if value == 0.0 && sections.len() >= 3 {
        (sections[2].as_str(), value)
    } else {
        (sections[0].as_str(), value)
    };

    let parts = match NumberFormat::parse(section) {
        Some(parts) => parts,
        // `0.00;-0.00;"none"` — a section that is only literal text.
        None if value == 0.0 => return literal_text(section),
        None => return None,
    };
    let mut number = parts.render(magnitude);
    if value < 0.0 && sections.len() < 2 {
        number.insert(0, '-');
    }
    Some(number)
}

/// Text of a format section that carries no numeric skeleton, if that is all it
/// holds.
fn literal_text(section: &str) -> Option<String> {
    let mut text = String::new();
    let mut characters = section.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '"' => {
                for next in characters.by_ref() {
                    if next == '"' {
                        break;
                    }
                    text.push(next);
                }
            }
            '\\' => text.push(characters.next()?),
            '_' | '*' => {
                characters.next();
            }
            ' ' => {}
            // Anything else is a format token, i.e. not a pure literal.
            _ => return None,
        }
    }
    (!text.is_empty()).then_some(text)
}

fn split_sections(code: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut bracketed = false;

    for character in code.chars() {
        match character {
            '"' => {
                quoted = !quoted;
                current.push(character);
            }
            '[' if !quoted => {
                bracketed = true;
                current.push(character);
            }
            ']' if !quoted => {
                bracketed = false;
                current.push(character);
            }
            ';' if !quoted && !bracketed => {
                sections.push(std::mem::take(&mut current));
            }
            _ => current.push(character),
        }
    }
    sections.push(current);
    sections
}

/// The literal parts and numeric skeleton of one format section.
struct NumberFormat {
    prefix: String,
    suffix: String,
    decimals: usize,
    required_decimals: usize,
    thousands: bool,
    percent: bool,
}

impl NumberFormat {
    fn parse(section: &str) -> Option<Self> {
        let mut prefix = String::new();
        let mut suffix = String::new();
        let mut skeleton = String::new();
        let mut percent = false;
        let mut characters = section.chars().peekable();
        let mut after_skeleton = false;

        while let Some(character) = characters.next() {
            match character {
                // Colour, condition and locale prefixes; `[$€-407]` also names
                // the currency symbol.
                '[' => {
                    let mut bracket = String::new();
                    for next in characters.by_ref() {
                        if next == ']' {
                            break;
                        }
                        bracket.push(next);
                    }
                    if let Some(currency) = bracket.strip_prefix('$') {
                        let symbol = currency.split('-').next().unwrap_or("");
                        if !symbol.is_empty() {
                            if after_skeleton {
                                suffix.push_str(symbol);
                            } else {
                                prefix.push_str(symbol);
                            }
                        }
                    }
                }
                '"' => {
                    let mut literal = String::new();
                    for next in characters.by_ref() {
                        if next == '"' {
                            break;
                        }
                        literal.push(next);
                    }
                    if after_skeleton {
                        suffix.push_str(&literal);
                    } else {
                        prefix.push_str(&literal);
                    }
                }
                '\\' => {
                    if let Some(escaped) = characters.next() {
                        if after_skeleton {
                            suffix.push(escaped);
                        } else {
                            prefix.push(escaped);
                        }
                    }
                }
                // `_` pads to the width of the next character and `*` repeats
                // it; both are column alignment, so neither renders.
                '_' | '*' => {
                    characters.next();
                }
                '%' => {
                    percent = true;
                    suffix.push('%');
                }
                '0' | '#' | '?' | '.' | ',' => {
                    after_skeleton = true;
                    skeleton.push(character);
                }
                'E' | 'e' => return None,
                _ => {
                    if after_skeleton {
                        suffix.push(character);
                    } else {
                        prefix.push(character);
                    }
                }
            }
        }

        if !skeleton.contains('0') && !skeleton.contains('#') {
            return None;
        }

        let (integer, fraction) = match skeleton.split_once('.') {
            Some((integer, fraction)) => (integer, fraction),
            None => (skeleton.as_str(), ""),
        };
        let decimals = fraction
            .chars()
            .filter(|c| matches!(c, '0' | '#' | '?'))
            .count();
        let required_decimals = fraction.chars().filter(|c| *c == '0').count();

        Some(Self {
            prefix,
            suffix,
            decimals,
            required_decimals,
            thousands: integer.contains(','),
            percent,
        })
    }

    fn render(&self, value: f64) -> String {
        let scaled = if self.percent { value * 100.0 } else { value };
        // Excel rounds halves away from zero; `{:.*}` would round half to even.
        let factor = 10f64.powi(self.decimals as i32);
        let rounded = (scaled * factor).round() / factor;
        let mut digits = format!("{:.*}", self.decimals, rounded);

        // `#` drops trailing zeros, `0` keeps them.
        if self.decimals > self.required_decimals {
            while digits.contains('.')
                && digits.ends_with('0')
                && decimal_places(&digits) > self.required_decimals
            {
                digits.pop();
            }
            if digits.ends_with('.') {
                digits.pop();
            }
        }

        if self.thousands {
            digits = group_thousands(&digits);
        }

        format!("{}{}{}", self.prefix, digits, self.suffix)
    }
}

fn decimal_places(number: &str) -> usize {
    number.split_once('.').map(|(_, f)| f.len()).unwrap_or(0)
}

fn group_thousands(number: &str) -> String {
    let (integer, fraction) = match number.split_once('.') {
        Some((integer, fraction)) => (integer.to_string(), format!(".{fraction}")),
        None => (number.to_string(), String::new()),
    };
    let mut grouped = String::with_capacity(integer.len() + integer.len() / 3);
    for (index, character) in integer.chars().enumerate() {
        if index > 0 && (integer.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(character);
    }
    grouped + &fraction
}

/// Decoded and unescaped attribute value.
fn attribute_value(attribute: &quick_xml::events::attributes::Attribute<'_>) -> String {
    attribute
        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
        .map(|value| value.into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_scales_and_keeps_decimals() {
        assert_eq!(render_number(0.15, "0%").as_deref(), Some("15%"));
        assert_eq!(render_number(0.1234, "0.0%").as_deref(), Some("12.3%"));
        assert_eq!(render_number(0.1234, "0.00%").as_deref(), Some("12.34%"));
    }

    #[test]
    fn currency_takes_its_symbol_from_the_format() {
        assert_eq!(
            render_number(1234.5, "$#,##0.00").as_deref(),
            Some("$1,234.50")
        );
        // `0.00` carries no grouping, so thousands stay unseparated.
        assert_eq!(
            render_number(1234.5, "0.00 [$€-407]").as_deref(),
            Some("1234.50 €")
        );
        assert_eq!(
            render_number(1234.5, "#,##0.00\" kr\"").as_deref(),
            Some("1,234.50 kr")
        );
    }

    #[test]
    fn thousands_and_optional_decimals() {
        assert_eq!(render_number(1234.5, "#,##0").as_deref(), Some("1,235"));
        assert_eq!(
            render_number(1234567.0, "#,##0").as_deref(),
            Some("1,234,567")
        );
        assert_eq!(
            render_number(1234.5, "#,##0.00").as_deref(),
            Some("1,234.50")
        );
        assert_eq!(render_number(1234.5678, "0.##").as_deref(), Some("1234.57"));
        assert_eq!(render_number(1234.0, "0.##").as_deref(), Some("1234"));
    }

    #[test]
    fn negative_and_zero_sections_apply() {
        assert_eq!(
            render_number(-1234.5, "$#,##0.00;($#,##0.00)").as_deref(),
            Some("($1,234.50)")
        );
        assert_eq!(
            render_number(0.0, "0.00;[Red]-0.00;\"zero\"").as_deref(),
            Some("zero")
        );
    }

    #[test]
    fn unrenderable_formats_fall_back() {
        assert_eq!(render_number(1.0, "General"), None);
        assert_eq!(render_number(1.0, "yyyy-mm-dd"), None);
        assert_eq!(render_number(1.0, "0.00E+00"), None);
        assert_eq!(render_number(1.0, "@"), None);
    }

    #[test]
    fn cell_references_become_positions() {
        assert_eq!(cell_position("A1"), Some((0, 0)));
        assert_eq!(cell_position("B3"), Some((2, 1)));
        assert_eq!(cell_position("AA10"), Some((9, 26)));
        assert_eq!(cell_position("$C$4"), Some((3, 2)));
        assert_eq!(cell_position("1A"), None);
    }
}
