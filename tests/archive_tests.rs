//! Zip expansion: member dispatch, provenance, nesting and bomb limits.

use std::io::Write;

use chunkr::prelude::*;
use serde_json::json;

/// Build a zip with the given members (deflate, so ratios are realistic).
fn zip_with(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, content) in members {
        writer.start_file(*name, options).unwrap();
        writer.write_all(content).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn text<'a>(name: &'a str, content: &str) -> (&'a str, Vec<u8>) {
    (name, content.as_bytes().to_vec())
}

fn margins() -> ArchiveLimits {
    ArchiveLimits::default()
}

fn unique_tmp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "chunkr_archive_{}_{}_{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp test dir");
    dir
}

#[test]
fn test_members_are_extracted_with_provenance() {
    let bytes = zip_with(&[
        text("notes/readme.md", "# Title\n\nBody."),
        text("data/sales.csv", "region,units\nNorth,120\n"),
        ("logo.png", vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 1, 2, 3]),
        ("__MACOSX/._notes", b"resource fork".to_vec()),
    ]);

    let documents =
        load_documents_from_bytes(&bytes, "zip", Some("/tmp/bundle.zip")).unwrap();

    let entries: Vec<&str> = documents
        .iter()
        .map(|doc| doc.metadata["archive_entry"].as_str().unwrap())
        .collect();
    assert_eq!(entries, vec!["notes/readme.md", "data/sales.csv"]);
    assert!(
        !entries.iter().any(|entry| entry.starts_with("__MACOSX")),
        "macOS resource forks are not documents"
    );

    let readme = &documents[0];
    assert!(readme.content.contains("# Title"));
    assert_eq!(readme.metadata["format"], json!("text"));
    assert_eq!(readme.metadata["file_extension"], json!("md"));
    assert_eq!(readme.metadata["file_name"], json!("readme.md"));
    assert_eq!(readme.metadata["archive_entries"], json!(4));
    assert_eq!(
        readme.metadata["archive_skipped"],
        json!(1),
        "an unreadable member is counted, not fatal"
    );
    assert_eq!(
        readme.metadata["source"],
        json!("/tmp/bundle.zip!notes/readme.md")
    );
}

#[test]
fn test_archive_paths_and_extensionless_payloads_expand() {
    let docx = docx_like_docx();
    let bytes = zip_with(&[
        text("plain", "no extension, plain text"),
        ("report.docx", docx),
    ]);

    let documents = load_documents_from_bytes(&bytes, "zip", None).unwrap();

    assert!(documents[0].content.contains("no extension"));
    assert_eq!(documents[0].metadata["archive_entry"], json!("plain"));
    assert!(
        !documents[0].metadata.contains_key("file_extension"),
        "a member without an extension has none to report"
    );

    // The docx member goes through the office extractor, not the text fallback.
    let report = documents
        .iter()
        .find(|doc| doc.metadata["archive_entry"] == json!("report.docx"))
        .expect("docx member extracted");
    assert_eq!(report.metadata["format"], json!("docx"));
    assert!(report.content.contains("Quarterly report"));
}

#[test]
fn test_directory_loader_routes_members_by_their_own_format() {
    let dir = unique_tmp_dir("routing");
    let bytes = zip_with(&[
        text("data/sales.csv", "region,units\nNorth,120\nSouth,98\n"),
        text("notes.md", "# Heading\n\nSome prose that is worth chunking."),
    ]);
    std::fs::write(dir.join("bundle.zip"), &bytes).unwrap();

    let chunks = DirectoryLoader::new()
        .with_chunk_size(200)
        .with_overlap(0)
        .load_and_chunk(&dir)
        .unwrap();

    assert!(!chunks.is_empty());
    assert!(
        chunks.iter().all(|chunk| chunk.metadata.contains_key("archive_entry")),
        "every chunk keeps its archive provenance"
    );

    // The CSV member is chunked as a table with its header repeated, the
    // Markdown member as Markdown: members are not flattened to plain text.
    let csv_chunk = chunks
        .iter()
        .find(|chunk| chunk.metadata["file_extension"] == json!("csv"))
        .expect("csv member chunked");
    assert!(csv_chunk.content.contains("region"), "{}", csv_chunk.content);

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_nested_archives_expand_to_the_depth_limit() {
    let inner = zip_with(&[text("deep.txt", "two levels down")]);
    let outer = zip_with(&[("inner.zip", inner.clone()), text("top.txt", "top level")]);

    let documents = load_documents_from_bytes(&outer, "zip", Some("outer.zip")).unwrap();
    let deep = documents
        .iter()
        .find(|doc| doc.content.contains("two levels down"))
        .expect("nested member extracted");
    assert_eq!(deep.metadata["archive_entry"], json!("deep.txt"));
    assert_eq!(deep.metadata["source"], json!("outer.zip!inner.zip!deep.txt"));

    // A third level is refused rather than expanded without bound.
    let third = zip_with(&[("second.zip", zip_with(&[("third.zip", inner)]))]);
    let error = load_documents_from_bytes(&third, "zip", Some("third.zip")).unwrap_err();
    assert!(
        error.to_string().contains("nested more than 2 deep"),
        "{error}"
    );
}

#[test]
fn test_entry_count_limit_is_enforced() {
    let members: Vec<(&str, Vec<u8>)> = (0..8)
        .map(|index| (Box::leak(format!("file_{index}.txt").into_boxed_str()) as &str, b"x".to_vec()))
        .collect();
    let bytes = zip_with(&members);

    let limits = ArchiveLimits {
        max_entries: 4,
        ..margins()
    };
    let error = expand_zip(&bytes, None, &limits, 0).unwrap_err();
    assert!(error.to_string().contains("entry limit"), "{error}");
}

#[test]
fn test_uncompressed_size_limit_is_enforced() {
    let bytes = zip_with(&[("big.txt", vec![b'a'; 4096]), text("small.txt", "ok")]);
    let limits = ArchiveLimits {
        max_uncompressed_bytes: 1024,
        ..margins()
    };

    let error = expand_zip(&bytes, None, &limits, 0).unwrap_err();
    assert!(
        error.to_string().contains("uncompressed content exceeds"),
        "{error}"
    );
}

#[test]
fn test_compression_ratio_limit_is_enforced() {
    // Compressible dummy content: a zip bomb in miniature.
    let bytes = zip_with(&[("bomb.txt", vec![b'a'; 4096])]);
    let limits = ArchiveLimits {
        max_compression_ratio: 2,
        ratio_floor_bytes: 1024,
        ..margins()
    };

    let error = expand_zip(&bytes, None, &limits, 0).unwrap_err();
    assert!(error.to_string().contains("expands"), "{error}");

    // Below the floor the ratio is not suspicious, so well-compressed small
    // files still expand.
    let limits = ArchiveLimits {
        max_compression_ratio: 2,
        ratio_floor_bytes: 1024 * 1024,
        ..margins()
    };
    assert_eq!(expand_zip(&bytes, None, &limits, 0).unwrap().len(), 1);
}

#[test]
fn test_corrupt_archive_reports_the_problem() {
    let error = load_documents_from_bytes(b"PK\x03\x04 not really", "zip", Some("broken.zip"))
        .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("broken.zip"), "{message}");
    assert!(message.contains("archive:"), "{message}");
}

#[test]
fn test_zip_is_a_supported_extension() {
    assert!(supported_extensions().iter().any(|ext| ext == "zip"));
    assert!(is_structured("zip"));
    assert!(is_structured("ZIP"));
}

/// A minimal docx written as a zip, built here so archive tests stay independent.
fn docx_like_docx() -> Vec<u8> {
    zip_with(&[
        (
            "[Content_Types].xml",
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.to_vec(),
        ),
        (
            "word/document.xml",
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Quarterly report</w:t></w:r></w:p></w:body></w:document>"#.to_vec(),
        ),
    ])
}
