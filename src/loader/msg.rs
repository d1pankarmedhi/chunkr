//! Outlook `.msg` messages.
//!
//! A `.msg` is a CFB (compound file) container, not MIME, so the EML path cannot
//! read it. `msg_parser` handles the property streams, RTF-compressed bodies and
//! attachments; this module maps the result onto the same document shape the EML
//! loader produces, so a mailbox exported either way chunks and filters the same.
//!
//! Attachments are not thrown away: a forwarded spreadsheet or PDF is dispatched
//! by its own extension, exactly like a zip member, so it becomes table or page
//! chunks with `attachment_name` pointing back at the mail it arrived in.

use serde_json::Value;

use crate::error::ChunkrError;
use crate::loader::base::source_metadata;
use crate::loader::text::clean_text;
use crate::loader::web::html_to_text;
use crate::structures::document::Document;

/// How deep embedded messages are followed (a mail attached to a mail …).
///
/// Bounded because an embedded message can embed another: the depth keeps a
/// pathological chain from turning one file into an unbounded walk.
const MAX_MSG_DEPTH: usize = 4;

/// Extracts headers and body text from an Outlook message.
#[derive(Debug, Clone, Default)]
pub struct MsgLoader {}

impl MsgLoader {
    pub fn new() -> Self {
        Self {}
    }

    /// Load one `.msg` file.
    pub fn load_documents(
        &self,
        bytes: &[u8],
        source: Option<&str>,
    ) -> Result<Vec<Document>, ChunkrError> {
        self.load_with_depth(bytes, source, 0)
    }

    fn load_with_depth(
        &self,
        bytes: &[u8],
        source: Option<&str>,
        depth: usize,
    ) -> Result<Vec<Document>, ChunkrError> {
        let message = msg_parser::Outlook::from_slice(bytes).map_err(|error| {
            ChunkrError::ParseError(format!("msg: unparseable message: {error}"))
        })?;

        let subject = message.subject.trim().to_string();
        let from = person(&message.sender);
        let to = people(&message.to);
        let cc = people(&message.cc);
        let date = [
            &message.message_delivery_time,
            &message.client_submit_time,
            &message.creation_time,
        ]
        .into_iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string();

        let mut header = String::new();
        for (label, value) in [
            ("Subject", &subject),
            ("From", &from),
            ("To", &to),
            ("Cc", &cc),
            ("Date", &date),
        ] {
            if !value.is_empty() {
                header.push_str(label);
                header.push_str(": ");
                header.push_str(value);
                header.push('\n');
            }
        }

        // Bodies arrive as plain text, as HTML, or as RTF that decompresses to HTML.
        let (body, body_source) = if !message.body.trim().is_empty() {
            (message.body.clone(), "text")
        } else {
            let html = if !message.html.is_empty() {
                message.html.clone()
            } else {
                message.html_from_rtf().unwrap_or_default()
            };
            if html.trim().is_empty() {
                (String::new(), "none")
            } else {
                (html_to_text(html.as_bytes())?, "html")
            }
        };

        let content = clean_text(&format!("{header}\n{body}"));
        if content.trim().is_empty() {
            return Err(ChunkrError::ParseError(
                "msg: message has no subject or body".to_string(),
            ));
        }

        let mut metadata = source_metadata(source);
        metadata.insert("format".to_string(), Value::from("outlook"));
        metadata.insert("body_source".to_string(), Value::from(body_source));
        if !subject.is_empty() {
            metadata.insert("subject".to_string(), Value::from(subject));
        }
        if !from.is_empty() {
            metadata.insert("from".to_string(), Value::from(from));
        }
        if !to.is_empty() {
            metadata.insert("to".to_string(), Value::from(to));
        }
        if !cc.is_empty() {
            metadata.insert("cc".to_string(), Value::from(cc));
        }
        if !date.is_empty() {
            metadata.insert("date".to_string(), Value::from(date));
        }
        if !message.message_class.is_empty() {
            metadata.insert("msg_class".to_string(), Value::from(message.message_class));
        }

        let attachments: Vec<String> = message
            .attachments
            .iter()
            .map(|attachment| attachment.file_name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();
        metadata.insert(
            "attachment_count".to_string(),
            Value::from(message.attachments.len()),
        );
        if !attachments.is_empty() {
            // Joined rather than a list: flat scalars are what vector-store
            // filters accept, and the count above is the machine-readable part.
            metadata.insert(
                "attachments".to_string(),
                Value::from(attachments.join(", ")),
            );
        }

        let mut documents = vec![Document::new(content, metadata)];

        // Dispatched like archive members: a `.xlsx` attached to a mail should
        // chunk as a table, not as bytes somebody has to open by hand.
        if depth >= MAX_MSG_DEPTH {
            return Ok(documents);
        }
        for (index, attachment) in message.attachments.iter().enumerate() {
            let extension = attachment
                .extension
                .trim_start_matches('.')
                .to_ascii_lowercase();
            if extension.is_empty() || attachment.payload_bytes.is_empty() {
                continue;
            }

            let name = if attachment.long_file_name.trim().is_empty() {
                attachment.file_name.trim()
            } else {
                attachment.long_file_name.trim()
            };
            let name = if name.is_empty() {
                format!("attachment-{}", index + 1)
            } else {
                name.to_string()
            };
            let chain = match source {
                Some(source) => format!("{source}!{name}"),
                None => name.clone(),
            };

            // An embedded message goes through this loader again, so the depth
            // rides along with it.
            let extracted = if extension == "msg" {
                self.load_with_depth(&attachment.payload_bytes, Some(&chain), depth + 1)
            } else {
                crate::loader::auto::extract_payload(
                    &attachment.payload_bytes,
                    &extension,
                    Some(&chain),
                    depth,
                )
            };

            match extracted {
                Ok(extracted) => {
                    for mut document in extracted {
                        document
                            .metadata
                            .insert("attachment_name".to_string(), Value::from(name.clone()));
                        document
                            .metadata
                            .insert("attachment_index".to_string(), Value::from(index));
                        document.metadata.insert(
                            "attachment_count".to_string(),
                            Value::from(message.attachments.len()),
                        );
                        if let Some(subject) = documents.first().map(|first| {
                            first
                                .metadata
                                .get("subject")
                                .cloned()
                                .unwrap_or(Value::Null)
                        }) {
                            if !subject.is_null() {
                                document
                                    .metadata
                                    .entry("subject".to_string())
                                    .or_insert(subject);
                            }
                        }
                        documents.push(document);
                    }
                }
                // An unreadable attachment (a signature image, a vCard) is not a
                // reason to lose the mail it came with.
                Err(_) => continue,
            }
        }

        Ok(documents)
    }
}

/// `Name <address>` for one person, however much of that the message carries.
fn person(person: &msg_parser::Person) -> String {
    let name = person.name.trim();
    match (name, person.email.trim()) {
        // Outlook often stores the address as the display name too.
        (name, email) if name.eq_ignore_ascii_case(email) => email.to_string(),
        (name, email) if !name.is_empty() && !email.is_empty() => format!("{name} <{email}>"),
        (_, email) if !email.is_empty() => email.to_string(),
        (name, _) => name.to_string(),
    }
}

fn people(people: &[msg_parser::Person]) -> String {
    people
        .iter()
        .map(person)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}
