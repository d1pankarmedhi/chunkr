use super::base::BaseChunker;

pub struct WordChunker {}
impl WordChunker {
    pub fn new() -> Self {
        Self {}
    }
}

impl BaseChunker<Vec<String>> for WordChunker {
    fn chunk_text(&self, text: &str, chunk_size: usize) -> Vec<String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let mut result = Vec::new();
        let mut temp_str = String::new();
        let mut word_count = 0;

        for word in words {
            if word_count >= chunk_size {
                result.push(temp_str.trim_end().to_string());
                temp_str.clear();
                word_count = 0;
            }
            if !temp_str.is_empty() {
                temp_str.push(' ');
            }
            temp_str.push_str(word);
            word_count += 1;
        }
        if !temp_str.is_empty() {
            result.push(temp_str);
        }
        result
    }
}
