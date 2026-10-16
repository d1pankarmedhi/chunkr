//! Loader coverage for non-PDF formats: spreadsheets, office documents,
//! EPUB, HTML, RTF, XML, notebooks, email and legacy text encodings.

use std::io::Write;
use std::path::PathBuf;

use chunkr::prelude::*;

fn archive(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, content) in entries {
        writer.start_file(*name, options).unwrap();
        writer.write_all(content.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn unique_tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "chunkr_formats_{name}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn xlsx_fixture() -> Vec<u8> {
    archive(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
<Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>
</Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#,
        ),
        (
            "xl/workbook.xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="Sales" sheetId="1" r:id="rId1"/></sheets>
</workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>
</Relationships>"#,
        ),
        (
            "xl/sharedStrings.xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="4" uniqueCount="4">
<si><t>Region</t></si><si><t>Units</t></si><si><t>North</t></si><si><t>South</t></si></sst>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>
<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row>
<row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>120</v></c></row>
<row r="3"><c r="A3" t="s"><v>3</v></c><c r="B3"><v>98</v></c></row>
</sheetData></worksheet>"#,
        ),
    ])
}

fn docx_fixture() -> Vec<u8> {
    archive(&[
        (
            "word/document.xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
<w:p><w:r><w:t>Quarterly report</w:t></w:r></w:p>
<w:p><w:r><w:t>Revenue grew by </w:t></w:r><w:r><w:t>12%</w:t></w:r></w:p>
<w:tbl><w:tr><w:tc><w:p><w:r><w:t>Region</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Units</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
</w:body></w:document>"#,
        ),
        (
            "_rels/.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#,
        ),
    ])
}

fn pptx_fixture() -> Vec<u8> {
    archive(&[
        (
            "ppt/slides/slide1.xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Opening slide</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#,
        ),
        (
            "ppt/slides/slide2.xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Second slide</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#,
        ),
        (
            "ppt/slides/slide10.xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Tenth slide</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#,
        ),
    ])
}

fn odt_fixture() -> Vec<u8> {
    archive(&[(
        "content.xml",
        r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0">
<office:body><office:text>
<text:h text:outline-level="1">Handbook</text:h>
<text:p>Chapter <text:span>one</text:span> body.</text:p>
<table:table><table:table-row><table:table-cell><text:p>A</text:p></table:table-cell><table:table-cell><text:p>B</text:p></table:table-cell></table:table-row></table:table>
</office:text></office:body></office:document-content>"#,
    )])
}

fn epub_fixture() -> Vec<u8> {
    archive(&[
        ("mimetype", "application/epub+zip"),
        (
            "META-INF/container.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
<rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#,
        ),
        (
            "OEBPS/content.opf",
            r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bookid">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Systems Book</dc:title><dc:creator>A. Author</dc:creator></metadata>
<manifest>
<item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
<item id="ch2" href="text/ch2.xhtml" media-type="application/xhtml+xml"/>
<item id="style" href="style.css" media-type="text/css"/>
</manifest>
<spine><itemref idref="ch1"/><itemref idref="ch2"/></spine>
</package>"#,
        ),
        (
            "OEBPS/ch1.xhtml",
            r#"<html><body><h1>First chapter</h1><p>Alpha text.</p></body></html>"#,
        ),
        (
            "OEBPS/text/ch2.xhtml",
            r#"<html><body><h1>Second chapter</h1><p>Beta text.</p></body></html>"#,
        ),
    ])
}

#[test]
fn test_xlsx_sheets_become_documents_with_metadata() {
    let documents = SpreadsheetLoader::new()
        .load_documents(&xlsx_fixture(), Some("sales.xlsx"))
        .unwrap();

    assert_eq!(documents.len(), 1);
    let sheet = &documents[0];
    assert_eq!(
        sheet.metadata.get("sheet_name").unwrap(),
        &serde_json::json!("Sales")
    );
    assert_eq!(
        sheet.metadata.get("sheet_index").unwrap(),
        &serde_json::json!(0)
    );
    assert_eq!(
        sheet.metadata.get("row_count").unwrap(),
        &serde_json::json!(2),
        "row_count counts data rows, the header is separate"
    );
    assert_eq!(
        sheet.metadata.get("column_count").unwrap(),
        &serde_json::json!(2)
    );
    assert!(sheet.content.contains("| Region | Units |"));
    assert!(sheet.content.contains("| North | 120 |"));
    assert!(sheet.content.contains("| South | 98 |"));
}

#[test]
fn test_xlsx_chunks_repeat_header_and_keep_sheet_metadata() {
    let dir = unique_tmp_dir("xlsx_route");
    std::fs::write(dir.join("sales.xlsx"), xlsx_fixture()).unwrap();

    let chunks = DirectoryLoader::new()
        .with_chunk_size(40)
        .with_overlap(0)
        .load_and_chunk(&dir)
        .unwrap();

    assert!(chunks.len() > 1);
    for chunk in &chunks {
        assert_eq!(
            chunk.metadata.get("sheet_name").unwrap(),
            &serde_json::json!("Sales")
        );
        assert_eq!(
            chunk.metadata.get("file_extension").unwrap(),
            &serde_json::json!("xlsx")
        );
        assert!(chunk.content.contains("Region"));
    }

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_docx_paragraphs_and_table_cells() {
    let bytes = docx_fixture();
    let documents = OfficeLoader::new()
        .load_documents(&bytes, "docx", Some("report.docx"))
        .unwrap();

    assert_eq!(documents.len(), 1);
    let content = &documents[0].content;
    assert!(content.contains("Quarterly report"));
    assert!(content.contains("Revenue grew by 12%"));
    assert!(content.contains("Region | Units"));
}

#[test]
fn test_pptx_slides_are_naturally_ordered() {
    let bytes = pptx_fixture();
    let documents = OfficeLoader::new()
        .load_documents(&bytes, "pptx", None)
        .unwrap();

    assert_eq!(documents.len(), 3);
    assert!(documents[0].content.contains("Opening slide"));
    assert!(documents[1].content.contains("Second slide"));
    assert!(documents[2].content.contains("Tenth slide"));
    assert_eq!(
        documents[2].metadata.get("slide_number").unwrap(),
        &serde_json::json!(3)
    );
    assert_eq!(
        documents[2].metadata.get("total_slides").unwrap(),
        &serde_json::json!(3)
    );
}

#[test]
fn test_odt_headings_and_table_cells() {
    let bytes = odt_fixture();
    let documents = OfficeLoader::new()
        .load_documents(&bytes, "odt", None)
        .unwrap();

    assert_eq!(documents.len(), 1);
    let content = &documents[0].content;
    assert!(content.contains("Handbook"));
    assert!(content.contains("Chapter one body."));
    assert!(content.contains("A | B"));
}

#[test]
fn test_epub_spine_order_and_metadata() {
    let bytes = epub_fixture();
    let documents = EpubLoader::new()
        .load_documents(&bytes, Some("book.epub"))
        .unwrap();

    assert_eq!(documents.len(), 2);
    assert!(documents[0].content.contains("First chapter"));
    assert!(documents[1].content.contains("Second chapter"));
    assert_eq!(
        documents[1].metadata.get("book_title").unwrap(),
        &serde_json::json!("Systems Book")
    );
    assert_eq!(
        documents[1].metadata.get("book_author").unwrap(),
        &serde_json::json!("A. Author")
    );
    assert_eq!(
        documents[1].metadata.get("chapter_href").unwrap(),
        &serde_json::json!("OEBPS/text/ch2.xhtml")
    );
}

#[test]
fn test_html_extraction_drops_scripts_and_styles() {
    let html = br#"<html><head><title>Doc</title><style>p{color:red}</style>
<script>alert("noise")</script></head>
<body><h1>Heading</h1><p>Body text.</p><ul><li>Item</li></ul></body></html>"#;

    let documents = load_documents_from_bytes(html, "html", None).unwrap();
    let content = &documents[0].content;
    assert!(content.contains("Heading"));
    assert!(content.contains("Body text."));
    assert!(content.contains("Item"));
    assert!(!content.contains("alert"));
    assert!(!content.contains("color:red"));
}

#[test]
fn test_rtf_control_words_escapes_and_unicode() {
    let rtf = r"{\rtf1\ansi\deff0{\fonttbl{\f0 Times;}}\f0\fs24 Hello \'e9t\'e9 \u233? \b bold\b0\par Second \tab line\par}";
    let documents = load_documents_from_bytes(rtf.as_bytes(), "rtf", None).unwrap();
    let content = &documents[0].content;

    assert!(content.contains("Hello été é"));
    assert!(content.contains("bold"));
    assert!(content.contains("Second"));
    assert!(!content.contains("fonttbl"));
    assert!(!content.contains("Times"));
}

#[test]
fn test_xml_text_extraction() {
    let xml = r#"<?xml version="1.0"?><catalog><book id="1"><title>Rust</title><author>Someone</author></book></catalog>"#;
    let documents = load_documents_from_bytes(xml.as_bytes(), "xml", None).unwrap();
    let content = &documents[0].content;

    assert!(content.contains("Rust"));
    assert!(content.contains("Someone"));
    assert!(!content.contains("<title>"));
}

#[test]
fn test_notebook_cells_are_fenced() {
    let notebook = r##"{"cells":[
{"cell_type":"markdown","source":["# Title\n","Some prose"]},
{"cell_type":"code","source":["print('hi')"],"outputs":[{"output_type":"stream","text":["hi\n"]}]}
]}"##;

    let documents = load_documents_from_bytes(notebook.as_bytes(), "ipynb", None).unwrap();
    let content = &documents[0].content;

    assert!(content.contains("# Title"));
    assert!(content.contains("```\nprint('hi')\n```"));
    assert!(content.contains("hi"));
}

#[test]
fn test_eml_headers_and_body() {
    let eml = b"From: Ada <ada@example.com>\r\nTo: Bob <bob@example.com>\r\nSubject: Status\r\nDate: Mon, 5 May 2025 10:00:00 +0000\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nBody line one.\r\nBody line two.\r\n";

    let documents = load_documents_from_bytes(eml, "eml", Some("note.eml")).unwrap();
    let document = &documents[0];

    assert_eq!(
        document.metadata.get("subject").unwrap(),
        &serde_json::json!("Status")
    );
    assert!(document
        .metadata
        .get("from")
        .unwrap()
        .as_str()
        .unwrap()
        .contains("ada@example.com"));
    assert!(document.content.contains("Subject: Status"));
    assert!(document.content.contains("Body line one."));
}

#[test]
fn test_mbox_splits_messages() {
    let mbox = b"From ada@example.com Mon May  5 10:00:00 2025\nFrom: ada@example.com\nSubject: One\n\nalpha\n\nFrom bob@example.com Mon May  5 11:00:00 2025\nFrom: bob@example.com\nSubject: Two\n\nbeta\n";

    let documents = load_documents_from_bytes(mbox, "mbox", None).unwrap();

    assert_eq!(documents.len(), 2);
    assert_eq!(
        documents[0].metadata.get("subject").unwrap(),
        &serde_json::json!("One")
    );
    assert_eq!(
        documents[1].metadata.get("message_index").unwrap(),
        &serde_json::json!(2)
    );
    assert!(documents[1].content.contains("beta"));
}

#[test]
fn test_legacy_encoding_text_is_decoded() {
    let latin1 = b"Caf\xe9 cr\xe8me\n";
    let documents = load_documents_from_bytes(latin1, "txt", None).unwrap();

    assert!(documents[0].content.contains("Café crème"));
}

#[test]
fn test_utf16_text_with_bom_is_decoded() {
    let mut bytes = vec![0xff, 0xfe];
    for unit in "héllo".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }

    let documents = load_documents_from_bytes(&bytes, "log", None).unwrap();
    assert!(documents[0].content.contains("héllo"));
}

#[test]
fn test_binary_payload_is_rejected() {
    let binary = [0x50, 0x4b, 0x03, 0x04, 0x00, 0x01, 0x02, 0x00];
    let error = load_documents_from_bytes(&binary, "unknown", None).unwrap_err();
    assert!(error.to_string().contains("binary payload"));
}

#[test]
fn test_supported_extensions_cover_new_formats() {
    let extensions = supported_extensions();
    for expected in [
        "pdf", "xlsx", "xls", "ods", "docx", "pptx", "odt", "epub", "html", "rtf", "eml", "mbox",
        "xml", "ipynb",
    ] {
        assert!(
            extensions.iter().any(|e| e == expected),
            "missing extension: {expected}"
        );
    }
}

#[test]
fn test_directory_loader_mixes_text_binary_and_skips_bad_files_leniently() {
    let dir = unique_tmp_dir("mixed");
    std::fs::write(dir.join("notes.md"), "# Title\n\nSome markdown body.").unwrap();
    std::fs::write(
        dir.join("page.html"),
        "<html><body><p>Web page</p></body></html>",
    )
    .unwrap();
    std::fs::write(dir.join("book.epub"), epub_fixture()).unwrap();
    std::fs::write(dir.join("broken.docx"), [0x00, 0x01, 0x02, 0x03]).unwrap();

    let loader = DirectoryLoader::new().with_chunk_size(200).with_overlap(20);
    let (chunks, errors) = loader.load_and_chunk_lenient(&dir);

    assert_eq!(errors.len(), 1);
    assert!(errors[0].0.ends_with("broken.docx"));
    assert!(chunks
        .iter()
        .any(|chunk| chunk.content.contains("Some markdown")));
    assert!(chunks
        .iter()
        .any(|chunk| chunk.content.contains("Web page")));
    assert!(chunks
        .iter()
        .any(|chunk| chunk.content.contains("First chapter")));

    let documents = loader.load_files_lenient(&dir).0;
    assert!(documents
        .iter()
        .any(|doc| doc.content.contains("Beta text.")));

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_extensionless_payloads_name_themselves() {
    // Zip containers are identified by their entries, magic-prefixed files by
    // their prefix — whatever the file is called, or not called.
    let docx = docx_fixture();
    let xlsx = xlsx_fixture();
    let pptx = pptx_fixture();
    let epub = epub_fixture();
    let rtf = br"{\rtf1\ansi hello}";

    for (label, bytes, expected_format) in [
        ("docx", docx.as_slice(), "docx"),
        ("xlsx", xlsx.as_slice(), "spreadsheet"),
        ("epub", epub.as_slice(), "epub"),
        ("rtf", rtf.as_slice(), "rtf"),
    ] {
        for extension in ["", "bin"] {
            let documents = load_documents_from_bytes(bytes, extension, Some(label))
                .unwrap_or_else(|error| panic!("{label} as .{extension}: {error}"));
            assert_eq!(
                documents[0].metadata["format"], expected_format,
                "{label} sniffed as .{extension}"
            );
        }
    }

    let documents = load_documents_from_bytes(&pptx, "bin", None).unwrap();
    assert_eq!(documents.len(), 3, "pptx sniffs to one document per slide");
    assert_eq!(documents[0].metadata["format"], "pptx");
}

#[test]
fn test_text_extensions_are_never_overridden_by_content() {
    // A known extension is authoritative: percentage-prefixed text stays text.
    let documents = load_documents_from_bytes(b"%PDF-1.4 not really a pdf", "txt", None).unwrap();
    assert_eq!(documents[0].content, "%PDF-1.4 not really a pdf");
    assert_eq!(documents[0].metadata["format"], "text");
}

#[test]
fn test_unknown_binary_still_errors_instead_of_guessing() {
    let bytes = [0x00u8, 0x01, 0x02, 0x03, 0xff, 0xfe];
    let error = load_documents_from_bytes(&bytes, "bin", Some("blob.bin")).unwrap_err();
    assert!(error.to_string().contains("binary payload"), "{error}");

    // A zip archive we cannot attribute is not guessed either.
    let plain_zip = archive(&[("notes.txt", "just a text file")]);
    let error = load_documents_from_bytes(&plain_zip, "", Some("bundle")).unwrap_err();
    assert!(error.to_string().contains("binary payload"), "{error}");
}

#[test]
fn test_malformed_jsonl_names_the_broken_line() {
    let payload = "{\"id\":1}\n{\"id\":2}\n{\"id\":\n{\"id\":4}\n";
    let error = load_documents_from_bytes(payload.as_bytes(), "jsonl", Some("db.jsonl")).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("db.jsonl"), "{message}");
    assert!(message.contains("line 3"), "{message}");

    // Blank lines are fine, and valid JSONL still loads.
    let documents =
        load_documents_from_bytes(b"{\"id\":1}\n\n{\"id\":2}\n", "ndjson", None).unwrap();
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].metadata["format"], "json");
}

#[test]
fn test_json_payloads_keep_the_tolerant_path() {
    // JSON documents may carry comments or other extensions of the syntax; only
    // JSONL, whose lines are machine-generated, is validated.
    let documents = load_documents_from_bytes(b"// config\n{\"a\":1}", "json", None).unwrap();
    assert_eq!(documents[0].metadata["format"], "json");
}

/// A worksheet with a title row, merged headers, a vertical merge, dates,
/// a missing cached value and a formula.
fn rich_xlsx_fixture() -> Vec<u8> {
    archive(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
<Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>
</Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#,
        ),
        (
            "xl/workbook.xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="Forecast" sheetId="1" r:id="rId1"/></sheets>
</workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>
</Relationships>"#,
        ),
        (
            "xl/sharedStrings.xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="6" uniqueCount="6">
<si><t>Q1 revenue by region</t></si><si><t>North</t></si><si><t>South</t></si><si><t>Units</t></si><si><t>Revenue</t></si><si><t>Total</t></si></sst>"#,
        ),
        (
            "xl/styles.xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<numFmts count="1"><numFmt numFmtId="164" formatCode="yyyy\-mm\-dd"/></numFmts>
<cellXfs count="2">
<xf numFmtId="0" fontId="0" fillId="0" borderId="0"/>
<xf numFmtId="164" fontId="0" fillId="0" borderId="0" applyNumberFormat="1"/>
</cellXfs>
</styleSheet>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetData>
<row r="1"><c r="A1" t="s"><v>0</v></c></row>
<row r="2"><c r="A2" t="s" s="1"><v>1</v></c><c r="B2" t="s"><v>3</v></c><c r="C2" t="s"><v>4</v></c></row>
<row r="3"><c r="A3" t="s"><v>2</v></c><c r="B3"><v>98</v></c><c r="C3"><f>B3*3</f></c></row>
<row r="4"><c r="A4" t="s"><v>5</v></c><c r="B4"><v>45000</v></c><c r="C4" s="1"><v>45000</v></c></row>
</sheetData>
<mergeCells count="2"><mergeCell ref="A1:C1"/><mergeCell ref="A3:A4"/></mergeCells>
</worksheet>"#,
        ),
    ])
}

#[test]
fn test_xlsx_title_row_becomes_metadata_and_leaves_the_header() {
    let documents = load_documents_from_bytes(&rich_xlsx_fixture(), "xlsx", Some("forecast.xlsx"))
        .unwrap();
    let sheet = &documents[0];

    assert_eq!(
        sheet.metadata.get("sheet_title").unwrap(),
        &serde_json::json!("Q1 revenue by region"),
        "a merged title above the header is metadata, not the header"
    );
    assert!(
        sheet.content.starts_with("| North | Units | Revenue |"),
        "the header row still becomes the header: {}",
        sheet.content
    );
    assert_eq!(sheet.metadata.get("row_count").unwrap(), &serde_json::json!(2));
}

#[test]
fn test_merged_rows_carry_their_label_and_formula_cells_are_shown() {
    let documents = load_documents_from_bytes(&rich_xlsx_fixture(), "xlsx", None).unwrap();
    let content = &documents[0].content;

    // The A3:A4 merge labels the row it spans, and a value already in the
    // region is never overwritten.
    assert!(content.contains("| South | 98 | =B3*3 |"), "{content}");
    assert!(content.contains("| Total | 45000 | 2023-03-15 |"), "{content}");
}

#[test]
fn test_dates_render_as_text_not_serial_numbers() {
    let documents = load_documents_from_bytes(&rich_xlsx_fixture(), "xlsx", None).unwrap();
    let content = &documents[0].content;

    assert!(
        content.contains("2023-03-15 |"),
        "a date-formatted cell must not render as 45000: {content}"
    );
    assert!(
        !content.contains("2023-03-15 00:00"),
        "a date without a time stays a date: {content}"
    );
    assert!(
        content.contains("| Total | 45000 |"),
        "an unformatted number keeps its serial meaning: {content}"
    );
}

#[test]
fn test_multi_level_headers_join_from_vertical_merges() {
    let bytes = archive(&[
        (
            "[Content_Types].xml",
            r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        ),
        (
            "xl/workbook.xml",
            r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Two" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/></Relationships>"#,
        ),
        (
            "xl/sharedStrings.xml",
            r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="4" uniqueCount="4"><si><t>Region</t></si><si><t>2024</t></si><si><t>Q1</t></si><si><t>Q2</t></si><si><t>North</t></si></sst>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>
<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row>
<row r="2"><c r="B2" t="s"><v>2</v></c><c r="C2" t="s"><v>3</v></c></row>
<row r="3"><c r="A3" t="s"><v>4</v></c><c r="B3"><v>10</v></c><c r="C3"><v>20</v></c></row>
</sheetData><mergeCells count="2"><mergeCell ref="A1:A2"/><mergeCell ref="B1:C1"/></mergeCells></worksheet>"#,
        ),
    ]);

    let documents = load_documents_from_bytes(&bytes, "xlsx", None).unwrap();
    let content = &documents[0].content;

    assert!(
        content.starts_with("| Region | 2024 / Q1 | 2024 / Q2 |"),
        "merged header levels join per column: {content}"
    );
    assert!(content.contains("| North | 10 | 20 |"), "{content}");
    assert_eq!(
        documents[0].metadata.get("header_rows").unwrap(),
        &serde_json::json!(2),
        "multi-level headers are recorded"
    );
    assert!(
        !documents[0].metadata.contains_key("sheet_title"),
        "a merged header is not a title"
    );
}

#[test]
fn test_single_column_sheets_keep_their_first_row_as_header() {
    let bytes = archive(&[
        (
            "[Content_Types].xml",
            r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        ),
        (
            "xl/workbook.xml",
            r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Names" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/></Relationships>"#,
        ),
        (
            "xl/sharedStrings.xml",
            r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="3" uniqueCount="3"><si><t>Name</t></si><si><t>Ada</t></si><si><t>Grace</t></si></sst>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>
<row r="1"><c r="A1" t="s"><v>0</v></c></row>
<row r="2"><c r="A2" t="s"><v>1</v></c></row>
<row r="3"><c r="A3" t="s"><v>2</v></c></row>
</sheetData></worksheet>"#,
        ),
    ]);

    let documents = load_documents_from_bytes(&bytes, "xlsx", None).unwrap();
    assert!(
        documents[0].content.starts_with("| Name |"),
        "a one-column sheet keeps its header: {}",
        documents[0].content
    );
    assert!(
        !documents[0].metadata.contains_key("sheet_title"),
        "one distinct value per row is not a title"
    );
}
