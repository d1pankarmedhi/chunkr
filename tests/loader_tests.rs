// Import the chunker module
use chunkr::loader::{base::BaseLoader, pdf::PDFLoader};

#[test]
fn test_pdfloader() {
    let file_path = "/home/home/Steve M. McConnell - Code Complete-See notes (1993).pdf";
    let pdf_loader = PDFLoader::new();
    let content = pdf_loader.load_from_file(file_path).unwrap();
    let content_len = content.len();
    println!("content length: {}", content.len());
    assert_eq!(content_len, 2071694);
}
