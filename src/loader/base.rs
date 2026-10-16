//! Shared loader plumbing: the legacy trait and source metadata.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

pub trait BaseLoader<Output> {
    // generic return type
    fn load_from_file(&self, path: &str) -> Output;
}

/// Identity metadata every loader attaches: where the payload came from.
///
/// `source` is the path or URL the bytes were read from, so path components are
/// only recorded when they are actually present.
pub fn source_metadata(source: Option<&str>) -> HashMap<String, Value> {
    let mut metadata = HashMap::new();
    let Some(source) = source else {
        return metadata;
    };
    metadata.insert("source".to_string(), Value::from(source));

    let path = Path::new(source);
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        metadata.insert("file_name".to_string(), Value::from(name));
    }
    if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
        metadata.insert(
            "file_extension".to_string(),
            Value::from(extension.to_ascii_lowercase()),
        );
    }
    metadata
}
