use chunkr::prelude::*;
use std::path::PathBuf;

fn unique_tmp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "chunkr_dir_loader_{}_{}_{}",
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

fn cleanup(dir: &std::path::Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// Dir with one good .md file and one invalid-UTF8 .md file.
fn mixed_dir(tag: &str) -> (PathBuf, PathBuf) {
    let dir = unique_tmp_dir(tag);
    let good = dir.join("good.md");
    let bad = dir.join("bad.md");
    std::fs::write(&good, "# Title\n\nSome readable content for chunking.\n")
        .expect("write good file");
    std::fs::write(&bad, [0xffu8, 0xfe, 0x00, 0x28]).expect("write bad file");
    (dir, bad)
}

#[test]
fn test_lenient_load_skips_bad_file() {
    let (dir, bad) = mixed_dir("lenient");
    let loader = DirectoryLoader::new();

    let (docs, errors) = loader.load_files_lenient(&dir);

    assert_eq!(docs.len(), 1, "good file should load");
    assert_eq!(errors.len(), 1, "exactly one error expected");
    assert_eq!(errors[0].0, bad, "error should name the bad path");

    cleanup(&dir);
}

#[test]
fn test_lenient_chunk_skips_bad_file() {
    let (dir, bad) = mixed_dir("lenient_chunk");
    let loader = DirectoryLoader::new();

    let (chunks, errors) = loader.load_and_chunk_lenient(&dir);

    assert!(!chunks.is_empty(), "good file should produce chunks");
    assert!(chunks.iter().all(|c| c
        .metadata
        .get("file_name")
        .and_then(|v| v.as_str())
        == Some("good.md")));
    assert_eq!(errors.len(), 1, "exactly one error expected");
    assert_eq!(errors[0].0, bad, "error should name the bad path");

    cleanup(&dir);
}

#[test]
fn test_strict_load_error_names_bad_file() {
    let (dir, bad) = mixed_dir("strict");
    let loader = DirectoryLoader::new();

    let err = loader
        .load_files(&dir)
        .expect_err("strict load should fail on invalid UTF-8");
    let msg = err.to_string();
    assert!(
        msg.contains(&bad.to_string_lossy().into_owned()),
        "error message should contain the bad file path, got: {}",
        msg
    );

    cleanup(&dir);
}

#[test]
#[cfg(unix)]
fn test_symlink_loop_is_skipped() {
    let dir = unique_tmp_dir("symlink");
    let sub = dir.join("sub");
    std::fs::create_dir_all(&sub).expect("create subdir");
    std::fs::write(sub.join("real.md"), "# Real\n\nContent here.\n").expect("write real file");
    std::os::unix::fs::symlink(&dir, sub.join("loop")).expect("create symlink loop");

    let loader = DirectoryLoader::new();
    let files = loader.collect_files(&dir).expect("collect should not hang");

    // The symlink itself must never appear, and the real file exactly once.
    assert!(!files.iter().any(|p| p.ends_with("loop")));
    let real_count = files.iter().filter(|p| p.ends_with("real.md")).count();
    assert_eq!(real_count, 1, "no duplicates, got: {:?}", files);

    cleanup(&dir);
}
