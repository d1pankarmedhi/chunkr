//! ASR weights: a pinned manifest, a cache, and a verified download.
//!
//! Bundling weights in the crate is not an option worth taking: crates.io caps a
//! crate at 10 MB (whisper's smallest model is 78 MB), every published wheel would
//! carry a model most users never call, and which model to run is a decision about
//! language, GPU and latency that only the caller can make. What *is* bundled is
//! the manifest — name, size and SHA-256 for each model — so [`pull`] fetches a
//! file it can verify and [`resolve`] never guesses.
//!
//! Downloading goes through `curl`, `wget` or PowerShell rather than a TLS stack:
//! the crate has no C dependency and no HTTP client, and every platform this works
//! on already ships one of those. The file is written to `.part`, checked against
//! the expected size and digest, then renamed — an interrupted download can never
//! look like a usable model.

use std::io::Read;
use std::path::{Path, PathBuf};

/// One pinned whisper.cpp ggml model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec {
    /// Short name used on the command line (`base.en`).
    pub name: &'static str,
    /// File name inside the cache directory.
    pub file: &'static str,
    /// Download location.
    pub url: &'static str,
    /// Expected SHA-256 of the file, as published by the host.
    pub sha256: &'static str,
    /// Expected size in bytes.
    pub bytes: u64,
    /// Whether the model handles languages other than English.
    pub multilingual: bool,
}

/// Models [`pull`] knows how to fetch, English-only first, then multilingual.
///
/// Sizes follow whisper.cpp's own quantisation choices; `large` and the q5/q8
/// builds are deliberately absent — a 1.5 GB download is not something a chunking
/// library should start on a user's behalf.
pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        name: "tiny.en",
        file: "ggml-tiny.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin",
        sha256: "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f",
        bytes: 77_704_715,
        multilingual: false,
    },
    ModelSpec {
        name: "base.en",
        file: "ggml-base.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin",
        sha256: "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002",
        bytes: 147_964_211,
        multilingual: false,
    },
    ModelSpec {
        name: "small.en",
        file: "ggml-small.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin",
        sha256: "c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d",
        bytes: 487_614_201,
        multilingual: false,
    },
    ModelSpec {
        name: "tiny",
        file: "ggml-tiny.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
        sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
        bytes: 77_691_713,
        multilingual: true,
    },
    ModelSpec {
        name: "base",
        file: "ggml-base.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
        bytes: 147_951_465,
        multilingual: true,
    },
    ModelSpec {
        name: "small",
        file: "ggml-small.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        bytes: 487_601_967,
        multilingual: true,
    },
];

/// Find a model by its short name.
pub fn spec(name: &str) -> Option<&'static ModelSpec> {
    let name = name.trim();
    MODELS.iter().find(|spec| spec.name == name)
}

/// Where models are cached.
///
/// `CHUNKR_MODEL_DIR` wins so containers and CI can mount one; otherwise the
/// platform cache directory, never the package directory.
pub fn cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CHUNKR_MODEL_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir.trim());
        }
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    if cfg!(target_os = "windows") {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(local).join("chunkr").join("models");
        }
    } else if cfg!(target_os = "macos") {
        return PathBuf::from(home)
            .join("Library")
            .join("Caches")
            .join("chunkr")
            .join("models");
    }
    match std::env::var("XDG_CACHE_HOME") {
        Ok(cache) if !cache.trim().is_empty() => PathBuf::from(cache).join("chunkr").join("models"),
        _ => PathBuf::from(home)
            .join(".cache")
            .join("chunkr")
            .join("models"),
    }
}

/// A model that is already on disk, if it is.
pub fn cached(spec: &ModelSpec) -> Option<PathBuf> {
    let path = cache_dir().join(spec.file);
    // Size is a cheap guard against a half-written file from an older version.
    match std::fs::metadata(&path) {
        Ok(metadata) if metadata.len() == spec.bytes => Some(path),
        _ => None,
    }
}

/// Resolve a name or path without downloading anything.
///
/// A path (`./my-model.bin`, or anything containing a separator) is used as given,
/// so a user with their own quantisation is never second-guessed.
pub fn resolve(name_or_path: &str) -> Result<PathBuf, String> {
    let candidate = Path::new(name_or_path);
    if candidate.is_file() || name_or_path.contains(std::path::MAIN_SEPARATOR) {
        return if candidate.is_file() {
            Ok(candidate.to_path_buf())
        } else {
            Err(format!("model file not found: {name_or_path}"))
        };
    }

    let spec = spec(name_or_path).ok_or_else(|| {
        let names: Vec<&str> = MODELS.iter().map(|spec| spec.name).collect();
        format!(
            "unknown model '{name_or_path}' (known: {}), or pass a path to a ggml file",
            names.join(", ")
        )
    })?;

    cached(spec).ok_or_else(|| {
        format!(
            "model '{}' is not downloaded — run `chunkr models pull {}` (or set \
             CHUNKR_WHISPER_MODEL to an existing ggml file)",
            spec.name, spec.name
        )
    })
}

/// Download a model if it is not cached, verifying size and SHA-256.
///
/// Returns the path to the verified file. Re-pulling a cached model is a no-op, so
/// callers can use this as "make sure it is there".
pub fn pull(name_or_path: &str) -> Result<PathBuf, String> {
    let candidate = Path::new(name_or_path);
    if candidate.is_file() {
        return Ok(candidate.to_path_buf());
    }

    let spec = spec(name_or_path).ok_or_else(|| {
        let names: Vec<&str> = MODELS.iter().map(|spec| spec.name).collect();
        format!(
            "unknown model '{name_or_path}' (known: {})",
            names.join(", ")
        )
    })?;

    if let Some(path) = cached(spec) {
        return Ok(path);
    }

    let dir = cache_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
    let target = dir.join(spec.file);
    let part = dir.join(format!("{}.part", spec.file));
    let _ = std::fs::remove_file(&part);

    eprintln!(
        "downloading {} ({:.0} MB) to {}",
        spec.name,
        spec.bytes as f64 / 1e6,
        target.display()
    );
    fetch(spec.url, &part)?;
    verify(&part, spec)?;
    std::fs::rename(&part, &target).map_err(|error| format!("cannot move into place: {error}"))?;
    Ok(target)
}

/// Fetch `url` to `path` with whichever downloader this machine has.
fn fetch(url: &str, path: &Path) -> Result<(), String> {
    let attempts: [(&str, Vec<String>); 3] = [
        (
            "curl",
            vec![
                "-fsSL".to_string(),
                "--retry".to_string(),
                "2".to_string(),
                "-o".to_string(),
                path.display().to_string(),
                url.to_string(),
            ],
        ),
        (
            "wget",
            vec![
                "-q".to_string(),
                "-O".to_string(),
                path.display().to_string(),
                url.to_string(),
            ],
        ),
        (
            "powershell",
            vec![
                "-NoProfile".to_string(),
                "-Command".to_string(),
                format!(
                    "Invoke-WebRequest -Uri '{url}' -OutFile '{}'",
                    path.display()
                ),
            ],
        ),
    ];

    for (program, args) in attempts {
        let output = match std::process::Command::new(program)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .output()
        {
            Ok(output) => output,
            // Not installed: try the next one.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("cannot run {program}: {error}")),
        };
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{program} failed ({}): {}",
            output.status,
            stderr.lines().last().unwrap_or("no output")
        ));
    }

    Err(format!(
        "no way to download: install curl or wget, or fetch {url} into {} yourself",
        cache_dir().display()
    ))
}

fn verify(path: &Path, spec: &ModelSpec) -> Result<(), String> {
    let size = std::fs::metadata(path)
        .map_err(|error| format!("cannot read downloaded file: {error}"))?
        .len();
    if size != spec.bytes {
        let _ = std::fs::remove_file(path);
        return Err(format!(
            "download is {size} bytes, expected {} — retry `chunkr models pull {}`",
            spec.bytes, spec.name
        ));
    }

    let digest = sha256(path)?;
    if digest != spec.sha256 {
        let _ = std::fs::remove_file(path);
        return Err(format!(
            "checksum mismatch for {}: got {digest}, expected {} — the download was \
             corrupted or the file changed upstream",
            spec.name, spec.sha256
        ));
    }
    Ok(())
}

fn sha256(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};

    let mut file = std::fs::File::open(path).map_err(|error| format!("cannot open: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("cannot read: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// The whisper.cpp binary to drive, if there is one.
///
/// `CHUNKR_WHISPER_BIN` wins; otherwise the common install names are searched on
/// `PATH`, which is what a `brew install whisper-cpp` or a source build leaves
/// behind.
pub fn find_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("CHUNKR_WHISPER_BIN") {
        if !path.trim().is_empty() {
            let path = PathBuf::from(path.trim());
            if path.is_file() {
                return Some(path);
            }
        }
    }

    let names: &[&str] = if cfg!(target_os = "windows") {
        &["whisper-cli.exe", "whisper-cpp.exe", "whisper.exe"]
    } else {
        &["whisper-cli", "whisper-cpp", "whisper"]
    };
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
