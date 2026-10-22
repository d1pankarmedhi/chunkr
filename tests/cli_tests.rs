use std::io::Write;
use std::process::{Command, Stdio};

fn chunkr_bin() -> String {
    env!("CARGO_BIN_EXE_chunkr").to_string()
}

fn run_chunkr(args: &[&str], stdin_text: &str) -> std::process::Output {
    let mut child = Command::new(chunkr_bin())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn chunkr binary");
    child
        .stdin
        .take()
        .expect("no stdin")
        .write_all(stdin_text.as_bytes())
        .expect("failed to write stdin");
    child.wait_with_output().expect("failed to wait on chunkr")
}

#[test]
fn test_cli_sentence_default_flags_succeed() {
    let input = "Dr. Smith arrived at noon. He gave a keynote. The crowd applauded loudly. Afterwards there was cake.";
    let out = run_chunkr(&["-s", "sentence"], input);
    assert!(
        out.status.success(),
        "sentence strategy failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("keynote"),
        "expected chunk output, got: {}",
        stdout
    );
}

#[test]
fn test_cli_paragraph_small_chunk_size_succeeds() {
    let input = "Para one.\n\nPara two.\n\nPara three.\n";
    let out = run_chunkr(&["-s", "paragraph", "-c", "100"], input);
    assert!(
        out.status.success(),
        "paragraph strategy failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Para one"),
        "expected chunk output, got: {}",
        stdout
    );
}

#[test]
fn test_cli_recursive_smoke() {
    let input = "First paragraph here.\n\nSecond paragraph here.\n";
    let out = run_chunkr(&["-s", "recursive"], input);
    assert!(
        out.status.success(),
        "recursive strategy failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.stdout.is_empty(), "expected non-empty chunk output");
}

#[test]
fn test_cli_chunks_each_unit_and_prints_its_origin() {
    let dir = std::env::temp_dir().join(format!(
        "chunkr_cli_units_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("events.jsonl");
    std::fs::write(&path, "{\"id\":1}\n{\"id\":2}\n{\"id\":3}\n").unwrap();
    let path = path.to_str().unwrap().to_string();

    // Text output names the unit a chunk came from.
    let out = run_chunkr(&["-s", "json", "-f", "text", &path], "");
    assert!(
        out.status.success(),
        "cli failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("=== events.jsonl · records 0-2 ==="),
        "unit header missing from text output: {stdout}"
    );

    // JSONL output keeps the record span on each chunk.
    let out = run_chunkr(&["-s", "json", "-f", "jsonl", &path], "");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("\"start_record\":0") && stdout.contains("\"end_record\":2"),
        "record span missing from chunk metadata: {stdout}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
