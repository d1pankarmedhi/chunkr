pub trait BaseLoader<Output> {
    // generic return type
    fn load_from_file(&self, path: &str) -> Output;
}
