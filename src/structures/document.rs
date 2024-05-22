use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug)]
pub struct Document {
    pub content: String,
    pub metadata: HashMap<String, Value>,
}
