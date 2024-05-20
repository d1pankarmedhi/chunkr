use super::base::BaseChunker;

pub struct CharacterChunker {}
impl CharacterChunker {
    pub fn new() -> Self {
        Self {}
    }
}

impl BaseChunker<Vec<String>> for CharacterChunker {
    fn chunk_text(&self, text: &str, chunk_size: usize) -> Vec<String> {
        let mut result: Vec<String> = Vec::new();
        let mut temp_str = String::new();
        for char in text.chars() {
            temp_str.push(char);
            if temp_str.len() >= chunk_size || char == text.chars().last().unwrap() {
                result.push(temp_str.clone());
                temp_str.clear();
            }
        }
        if !temp_str.is_empty() {
            result.push(temp_str);
        }

        result
    }
}
