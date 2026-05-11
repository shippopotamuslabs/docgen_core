use serde::{Deserialize, Serialize};
use schemars::{schema_for, JsonSchema};
use std::io::Cursor;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum DocGenError {
    #[error("Validation error: {0}")]
    ValidationError(String),
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Generation error: {0}")]
    GenerationError(String),
    #[error("Extraction error: {0}")]
    ExtractionError(String),
    #[error("Unsupported mime type: {0}")]
    UnsupportedMimeType(String),
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct Document {
    pub version: String,
    pub metadata: Metadata,
    pub blocks: Vec<Block>,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct Metadata {
    pub title: String,
    pub author: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Heading { level: u32, text: String },
    Paragraph { text: String },
    List { ordered: bool, items: Vec<String> },
    Table {
        header: Vec<String>,
        rows: Vec<Vec<String>>,
        #[serde(default)]
        alignments: Option<Vec<Alignment>>,
    },
    Code { language: Option<String>, text: String },
    Math { display: bool, latex: String },
    Image { src: String, alt: Option<String> },
    PageBreak,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum Alignment {
    Left,
    Center,
    Right,
}

#[uniffi::export]
pub fn get_schema() -> String {
    let schema = schema_for!(Document);
    serde_json::to_string_pretty(&schema).unwrap()
}

#[uniffi::export]
pub fn validate_document(json: String) -> Result<(), DocGenError> {
    let schema_json = schema_for!(Document);
    let schema_value = serde_json::to_value(&schema_json).map_err(|e| DocGenError::SerializationError(e.to_string()))?;
    
    let validator = jsonschema::validator_for(&schema_value).map_err(|e| DocGenError::ValidationError(e.to_string()))?;
    
    let instance: serde_json::Value = serde_json::from_str(&json).map_err(|e| DocGenError::SerializationError(e.to_string()))?;
    
    let error_messages: Vec<String> = validator
        .iter_errors(&instance)
        .map(|e| e.to_string())
        .collect();
        
    if !error_messages.is_empty() {
        return Err(DocGenError::ValidationError(error_messages.join(", ")));
    }
    
    // Also try to deserialize to make sure it's fully valid
    let _: Document = serde_json::from_str(&json).map_err(|e| DocGenError::SerializationError(e.to_string()))?;
    
    Ok(())
}

#[uniffi::export]
pub fn generate_docx(json: String) -> Result<Vec<u8>, DocGenError> {
    let doc: Document = serde_json::from_str(&json).map_err(|e| DocGenError::SerializationError(e.to_string()))?;
    let mut docx = docx_rs::Docx::new();
    
    for block in doc.blocks {
        match block {
            Block::Heading { level, text } => {
                let mut p = docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text(text));
                match level {
                    1 => p = p.style("Heading1"),
                    2 => p = p.style("Heading2"),
                    3 => p = p.style("Heading3"),
                    _ => p = p.style("Heading4"),
                }
                docx = docx.add_paragraph(p);
            }
            Block::Paragraph { text } => {
                docx = docx.add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text(text)));
            }
            Block::List { ordered, items } => {
                for (i, item) in items.iter().enumerate() {
                    let mut p = docx_rs::Paragraph::new()
                        .add_run(docx_rs::Run::new().add_text(item));
                    if ordered {
                        p = p.add_run(docx_rs::Run::new().add_text(format!("{}. ", i + 1)));
                    } else {
                        p = p.add_run(docx_rs::Run::new().add_text("- "));
                    }
                    docx = docx.add_paragraph(p);
                }
            }
            Block::Table { header, rows, .. } => {
                let mut t = docx_rs::Table::new(vec![]);
                
                // Add header
                let mut header_cells = vec![];
                for h in header {
                    header_cells.push(docx_rs::TableCell::new().add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text(h))));
                }
                t = t.add_row(docx_rs::TableRow::new(header_cells));
                
                // Add rows
                for r in rows {
                    let mut cells = vec![];
                    for c in r {
                        cells.push(docx_rs::TableCell::new().add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text(c))));
                    }
                    t = t.add_row(docx_rs::TableRow::new(cells));
                }
                docx = docx.add_table(t);
            }
            Block::Code { text, .. } => {
                docx = docx.add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text(text)));
            }
            Block::Math { latex, .. } => {
                docx = docx.add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text(latex)));
            }
            Block::Image { src, alt } => {
                let alt_text = alt.unwrap_or_else(|| "Image".to_string());
                docx = docx.add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text(format!("[Image: {} (src: {})]", alt_text, src))));
            }
            Block::PageBreak => {
                docx = docx.add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_break(docx_rs::BreakType::Page)));
            }
        }
    }
    
    let mut buf = Vec::new();
    docx.build().pack(Cursor::new(&mut buf)).map_err(|e| DocGenError::GenerationError(e.to_string()))?;
    Ok(buf)
}

#[uniffi::export]
pub fn generate_pdf(json: String) -> Result<Vec<u8>, DocGenError> {
    let doc: Document = serde_json::from_str(&json).map_err(|e| DocGenError::SerializationError(e.to_string()))?;
    
    let (pdf_doc, page1, layer1) = printpdf::PdfDocument::new(&doc.metadata.title, printpdf::Mm(210.0), printpdf::Mm(297.0), "Layer 1");
    let current_layer = pdf_doc.get_page(page1).get_layer(layer1);
    
    let font = pdf_doc.add_builtin_font(printpdf::BuiltinFont::Helvetica).map_err(|e| DocGenError::GenerationError(e.to_string()))?;
    
    let mut y = 280.0;
    current_layer.use_text(&doc.metadata.title, 24.0, printpdf::Mm(20.0), printpdf::Mm(y), &font);
    y -= 20.0;
    
    if let Some(author) = &doc.metadata.author {
        current_layer.use_text(format!("By {}", author), 12.0, printpdf::Mm(20.0), printpdf::Mm(y), &font);
        y -= 10.0;
    }
    
    for block in doc.blocks {
        if y < 20.0 {
            break;
        }
        match block {
            Block::Heading { text, .. } => {
                current_layer.use_text(text, 18.0, printpdf::Mm(20.0), printpdf::Mm(y), &font);
                y -= 15.0;
            }
            Block::Paragraph { text } => {
                current_layer.use_text(text, 12.0, printpdf::Mm(20.0), printpdf::Mm(y), &font);
                y -= 10.0;
            }
            Block::List { items, .. } => {
                for item in items {
                    current_layer.use_text(format!("• {}", item), 12.0, printpdf::Mm(25.0), printpdf::Mm(y), &font);
                    y -= 8.0;
                }
            }
            Block::Table { header, rows, .. } => {
                let header_str = header.join(" | ");
                current_layer.use_text(header_str, 12.0, printpdf::Mm(20.0), printpdf::Mm(y), &font);
                y -= 10.0;
                for row in rows {
                    let row_str = row.join(" | ");
                    current_layer.use_text(row_str, 10.0, printpdf::Mm(20.0), printpdf::Mm(y), &font);
                    y -= 8.0;
                }
            }
            _ => {
            }
        }
    }
    
    let buf = pdf_doc.save_to_bytes().map_err(|e| DocGenError::GenerationError(e.to_string()))?;
    Ok(buf)
}

#[uniffi::export]
pub fn extract_text(bytes: Vec<u8>, mime_type: String) -> Result<String, DocGenError> {
    match mime_type.as_str() {
        "application/pdf" => {
            let doc = lopdf::Document::load_mem(&bytes).map_err(|e| DocGenError::ExtractionError(e.to_string()))?;
            let mut text = String::new();
            for page in doc.get_pages() {
                let page_text = doc.extract_text(&[page.0]).map_err(|e| DocGenError::ExtractionError(e.to_string()))?;
                text.push_str(&page_text);
                text.push('\n');
            }
            Ok(text)
        }
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
            Ok("DOCX extraction not fully implemented".to_string())
        }
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => {
            use calamine::{Reader, Xlsx, Data};
            let cursor = Cursor::new(bytes);
            let mut excel: Xlsx<_> = calamine::open_workbook_from_rs(cursor).map_err(|e: calamine::XlsxError| DocGenError::ExtractionError(e.to_string()))?;
            let mut text = String::new();
            if let Some(Ok(range)) = excel.worksheet_range_at(0) {
                for row in range.rows() {
                    for cell in row {
                        match cell {
                            Data::Empty => {},
                            Data::String(s) => text.push_str(&format!("{} ", s)),
                            Data::Float(f) => text.push_str(&format!("{} ", f)),
                            Data::Int(i) => text.push_str(&format!("{} ", i)),
                            Data::Bool(b) => text.push_str(&format!("{} ", b)),
                            Data::Error(e) => text.push_str(&format!("Error({:?}) ", e)),
                            Data::DateTime(d) => text.push_str(&format!("{} ", d)),
                            Data::DateTimeIso(d) => text.push_str(&format!("{} ", d)),
                            Data::DurationIso(d) => text.push_str(&format!("{} ", d)),
                        }
                    }
                    text.push('\n');
                }
            }
            Ok(text)
        }
        _ => Err(DocGenError::UnsupportedMimeType(mime_type)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get_sample_json() -> String {
        r#"{
            "version": "1.0",
            "metadata": { "title": "Test Doc", "author": "Sean" },
            "blocks": [
                { "type": "heading", "level": 1, "text": "Project Phoenix" },
                { "type": "paragraph", "text": "This is a test paragraph." },
                { "type": "list", "ordered": true, "items": ["Item 1", "Item 2"] },
                { "type": "table", "header": ["H1", "H2"], "rows": [["R1C1", "R1C2"], ["R2C1", "R2C2"]] },
                { "type": "code", "language": "rust", "text": "fn main() {}" },
                { "type": "page_break" }
            ]
        }"#.to_string()
    }

    #[test]
    fn test_get_schema() {
        let schema = get_schema();
        assert!(schema.contains("\"$schema\""));
        assert!(schema.contains("\"Document\""));
        assert!(schema.contains("\"heading\""));
        assert!(schema.contains("\"paragraph\""));
    }

    #[test]
    fn test_validate_document_success() {
        let valid_json = get_sample_json();
        assert!(validate_document(valid_json).is_ok());
    }

    #[test]
    fn test_validate_document_failure_invalid_type() {
        let invalid_json = r#"{
            "version": "1.0",
            "metadata": { "title": "Test Doc" },
            "blocks": [
                { "type": "invalid_type", "text": "oops" }
            ]
        }"#.to_string();
        let result = validate_document(invalid_json);
        assert!(result.is_err());
        match result.unwrap_err() {
            DocGenError::ValidationError(msg) => assert!(msg.contains("invalid_type")),
            _ => panic!("Expected ValidationError"),
        }
    }

    #[test]
    fn test_validate_document_failure_missing_fields() {
        let invalid_json = r#"{
            "version": "1.0",
            "metadata": { "title": "Test Doc" },
            "blocks": [
                { "type": "heading", "text": "missing level" }
            ]
        }"#.to_string();
        let result = validate_document(invalid_json);
        assert!(result.is_err());
        match result.unwrap_err() {
            DocGenError::ValidationError(msg) => assert!(msg.contains("level")),
            _ => panic!("Expected ValidationError"),
        }
    }

    #[test]
    fn test_generate_docx() {
        let json = get_sample_json();
        let result = generate_docx(json);
        assert!(result.is_ok());
        let bytes = result.unwrap();
        assert!(!bytes.is_empty());
        // DOCX files are zip files, should start with PK
        assert_eq!(&bytes[0..2], b"PK");
    }

    #[test]
    fn test_generate_pdf() {
        let json = get_sample_json();
        let result = generate_pdf(json);
        assert!(result.is_ok());
        let bytes = result.unwrap();
        assert!(!bytes.is_empty());
        // PDF files start with %PDF
        assert_eq!(&bytes[0..4], b"%PDF");
    }

    #[test]
    fn test_extract_text_unsupported() {
        let bytes = vec![1, 2, 3];
        let result = extract_text(bytes, "image/png".to_string());
        assert!(result.is_err());
        match result.unwrap_err() {
            DocGenError::UnsupportedMimeType(mime) => assert_eq!(mime, "image/png"),
            _ => panic!("Expected UnsupportedMimeType"),
        }
    }
}
