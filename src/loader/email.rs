//! Email extraction for EML messages and MBOX mailboxes.

use serde_json::Value;

use crate::error::ChunkrError;
use crate::loader::base::source_metadata;
use crate::loader::text::clean_text;
use crate::loader::web::html_to_text;
use crate::structures::document::Document;

/// Extracts headers and body text from email messages.
#[derive(Debug, Clone, Default)]
pub struct EmailLoader {}

impl EmailLoader {
    pub fn new() -> Self {
        Self {}
    }

    /// Load a single `.eml` message.
    pub fn load_documents(
        &self,
        bytes: &[u8],
        extension: &str,
        source: Option<&str>,
    ) -> Result<Vec<Document>, ChunkrError> {
        match extension.to_ascii_lowercase().as_str() {
            "mbox" => Ok(self.mbox(bytes, source)),
            _ => Ok(vec![self.message(bytes, source, None)?]),
        }
    }

    /// Parse one message, optionally tagged with its position inside a mailbox.
    fn message(
        &self,
        raw: &[u8],
        source: Option<&str>,
        message_index: Option<usize>,
    ) -> Result<Document, ChunkrError> {
        let parsed = mail_parser::MessageParser::default()
            .parse(raw)
            .ok_or_else(|| ChunkrError::ParseError("email: unparseable message".into()))?;

        let mut header = String::new();
        let subject = parsed.subject().unwrap_or_default().to_string();
        let from = parsed.from().map(address_list).unwrap_or_default();
        let to = parsed.to().map(address_list).unwrap_or_default();
        let date = parsed
            .date()
            .map(|date| date.to_rfc3339())
            .unwrap_or_default();
        let message_id = parsed.message_id().unwrap_or_default().to_string();

        for (label, value) in [
            ("Subject", &subject),
            ("From", &from),
            ("To", &to),
            ("Date", &date),
        ] {
            if !value.is_empty() {
                header.push_str(label);
                header.push_str(": ");
                header.push_str(value);
                header.push('\n');
            }
        }

        let body = match parsed.body_text(0) {
            Some(text) => text.into_owned(),
            None => match parsed.body_html(0) {
                Some(html) => html_to_text(html.as_bytes())?,
                None => String::new(),
            },
        };

        let content = clean_text(&format!("{header}\n{body}"));
        let mut metadata = source_metadata(source);
        metadata.insert("format".to_string(), Value::from("email"));
        if !subject.is_empty() {
            metadata.insert("subject".to_string(), Value::from(subject));
        }
        if !from.is_empty() {
            metadata.insert("from".to_string(), Value::from(from));
        }
        if !to.is_empty() {
            metadata.insert("to".to_string(), Value::from(to));
        }
        if !date.is_empty() {
            metadata.insert("date".to_string(), Value::from(date));
        }
        if !message_id.is_empty() {
            metadata.insert("message_id".to_string(), Value::from(message_id));
        }
        if let Some(index) = message_index {
            metadata.insert("message_index".to_string(), Value::from(index));
        }

        Ok(Document::new(content, metadata))
    }

    /// Split a mailbox into messages using the `From ` separator convention.
    fn mbox(&self, bytes: &[u8], source: Option<&str>) -> Vec<Document> {
        let messages = mail_parser::mailbox::mbox::MessageIterator::new(bytes);
        let mut documents = Vec::new();

        for (index, message) in messages.enumerate() {
            let Ok(message) = message else { continue };
            if let Ok(document) = self.message(message.contents(), source, Some(index + 1)) {
                documents.push(document);
            }
        }

        documents
    }
}

/// Render a header address field as `Name <addr>, …`.
fn address_list(address: &mail_parser::Address<'_>) -> String {
    use mail_parser::{Addr, Address, Group};

    fn single(addr: &Addr<'_>) -> String {
        match (addr.name.as_deref(), addr.address.as_deref()) {
            (Some(name), Some(email)) => format!("{name} <{email}>"),
            (None, Some(email)) => email.to_string(),
            (Some(name), None) => name.to_string(),
            (None, None) => String::new(),
        }
    }

    fn group(group: &Group<'_>) -> String {
        let members = group
            .addresses
            .iter()
            .map(single)
            .collect::<Vec<_>>()
            .join(", ");
        match group.name.as_deref() {
            Some(name) => format!("{name}: {members}"),
            None => members,
        }
    }

    match address {
        Address::List(list) => list.iter().map(single).collect::<Vec<_>>().join(", "),
        Address::Group(groups) => groups.iter().map(group).collect::<Vec<_>>().join("; "),
    }
}
