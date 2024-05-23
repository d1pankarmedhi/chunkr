use super::base::BaseLoader;
use lopdf::Document;
use std::io::Result;

pub struct PDFLoader {}
impl PDFLoader {
    pub fn new() -> Self {
        Self {}
    }
}

impl BaseLoader<Result<String>> for PDFLoader {
    fn load_from_file(&self, path: &str) -> Result<String> {
        let doc = Document::load(path).unwrap();
        let mut content = String::new();

        let pages = doc.get_pages();
        for (i, _) in pages.iter().enumerate() {
            let page_number = (i + 1) as u32;
            let text = doc.extract_text(&[page_number]).unwrap();
            content.push_str(text.as_str());
        }
        Ok(content)
    }
}
