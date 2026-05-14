use serde::{Deserialize, Serialize};
use schemars::{schema_for, JsonSchema};
use std::io::Cursor;

uniffi::setup_scaffolding!();

mod md_render;
mod pdf_gen;
mod pdf_metrics;
mod highlight;
mod docx_impl;

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
    Heading {
        level: u32,
        text: String,
        #[serde(default)]
        anchor: Option<String>,
    },
    Paragraph {
        text: String,
        #[serde(default)]
        align: Option<Alignment>,
    },
    List {
        ordered: bool,
        items: Vec<ListItem>,
        #[serde(default)]
        start: Option<u32>,
    },
    Table {
        header: Vec<String>,
        rows: Vec<Vec<String>>,
        #[serde(default)]
        alignments: Option<Vec<Alignment>>,
        #[serde(default)]
        caption: Option<String>,
    },
    Code {
        #[serde(default)]
        language: Option<String>,
        text: String,
        #[serde(default)]
        show_line_numbers: bool,
        #[serde(default)]
        caption: Option<String>,
        #[serde(default)]
        highlight_lines: Vec<u32>,
    },
    Math { display: bool, latex: String },
    Image {
        src: String,
        #[serde(default)]
        alt: Option<String>,
        #[serde(default)]
        data_base64: Option<String>,
        #[serde(default)]
        mime: Option<String>,
        #[serde(default)]
        width_pt: Option<f32>,
        #[serde(default)]
        height_pt: Option<f32>,
        #[serde(default)]
        align: Option<Alignment>,
    },
    PageBreak,
    Blockquote {
        text: String,
        #[serde(default)]
        attribution: Option<String>,
    },
    HorizontalRule,
    Callout {
        kind: CalloutKind,
        #[serde(default)]
        title: Option<String>,
        text: String,
    },
    Footnote { id: String, text: String },
    Toc {
        #[serde(default)]
        max_depth: Option<u32>,
    },
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct ListItem {
    pub text: String,
    #[serde(default)]
    pub checked: Option<bool>,
    #[serde(default)]
    pub children: Vec<ListItem>,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CalloutKind {
    Note,
    Info,
    Tip,
    Warning,
    Danger,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Alignment {
    Left,
    Center,
    Right,
}

/// LLM tool arguments for `start_artifact` (camelCase JSON, unknown fields rejected).
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartArtifactParams {
    pub title: String,
    #[serde(default)]
    pub author: Option<String>,
}

/// LLM tool arguments for `append_blocks` (camelCase JSON, unknown fields rejected).
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppendBlocksParams {
    pub artifact_id: String,
    pub blocks: Vec<Block>,
    pub is_complete: bool,
}

#[uniffi::export]
pub fn get_start_artifact_schema() -> String {
    let schema = schema_for!(StartArtifactParams);
    serde_json::to_string_pretty(&schema).unwrap()
}

#[uniffi::export]
pub fn get_append_blocks_schema() -> String {
    let schema = schema_for!(AppendBlocksParams);
    serde_json::to_string_pretty(&schema).unwrap()
}

#[uniffi::export]
pub fn validate_start_artifact_params(json: String) -> Result<(), DocGenError> {
    serde_json::from_str::<StartArtifactParams>(&json).map_err(|e| {
        DocGenError::ValidationError(format!("start_artifact arguments: {e}"))
    })?;
    Ok(())
}

#[uniffi::export]
pub fn validate_append_blocks_params(json: String) -> Result<(), DocGenError> {
    serde_json::from_str::<AppendBlocksParams>(&json).map_err(|e| {
        DocGenError::ValidationError(format!("append_blocks arguments: {e}"))
    })?;
    Ok(())
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
    docx_impl::build_docx_bytes(&doc)
}

#[uniffi::export]
pub fn generate_pdf(json: String) -> Result<Vec<u8>, DocGenError> {
    let doc: Document = serde_json::from_str(&json).map_err(|e| DocGenError::SerializationError(e.to_string()))?;
    pdf_gen::generate_pdf_from_document(&doc)
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
    use std::io::Read;

    fn get_sample_json() -> String {
        r#"{
            "version": "1.0",
            "metadata": { "title": "Test Doc", "author": "Sean" },
            "blocks": [
                { "type": "heading", "level": 1, "text": "Project Phoenix" },
                { "type": "paragraph", "text": "This is a test paragraph." },
                { "type": "list", "ordered": true, "items": [{"text": "Item 1", "checked": null, "children": []}, {"text": "Item 2", "checked": null, "children": []}] },
                { "type": "table", "header": ["H1", "H2"], "rows": [["R1C1", "R1C2"], ["R2C1", "R2C2"]] },
                { "type": "code", "language": "rust", "text": "fn main() {}" },
                { "type": "page_break" }
            ]
        }"#.to_string()
    }

    #[test]
    fn test_pdf_paginates_long_content() {
        let mut blocks = Vec::new();
        for i in 0..90 {
            blocks.push(Block::Paragraph {
                text: format!(
                    "Paragraph {}. Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
                     Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris. ",
                    i
                ),
                align: None,
            });
        }
        let doc = Document {
            version: "1.0".to_string(),
            metadata: Metadata {
                title: "Pagination test".to_string(),
                author: None,
            },
            blocks,
        };
        let json = serde_json::to_string(&doc).unwrap();
        let pdf_bytes = generate_pdf(json).unwrap();
        let lopdf_doc = lopdf::Document::load_mem(&pdf_bytes).unwrap();
        assert!(
            lopdf_doc.get_pages().len() >= 2,
            "expected automatic pagination, got {} page(s)",
            lopdf_doc.get_pages().len()
        );
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
    fn test_start_artifact_schema_has_title_and_optional_author() {
        let s = get_start_artifact_schema();
        assert!(s.contains("\"title\""));
        assert!(s.contains("\"author\""));
        assert!(s.contains("\"required\""));
        assert!(s.contains("\"title\"") && s.contains("StartArtifactParams"));
    }

    #[test]
    fn test_append_blocks_schema_has_artifact_id_blocks_is_complete() {
        let s = get_append_blocks_schema();
        assert!(s.contains("\"artifactId\"") || s.contains("\"artifact_id\""));
        assert!(s.contains("\"blocks\""));
        assert!(s.contains("\"isComplete\"") || s.contains("\"is_complete\""));
        assert!(s.contains("heading") || s.contains("Heading"));
    }

    #[test]
    fn test_validate_start_artifact_params_ok() {
        assert!(validate_start_artifact_params(r#"{"title":"T"}"#.to_string()).is_ok());
        assert!(validate_start_artifact_params(r#"{"title":"T","author":"A"}"#.to_string()).is_ok());
    }

    #[test]
    fn test_validate_start_artifact_params_rejects_document_shape() {
        let bad = r#"{"version":"1.0","metadata":{"title":"x"},"blocks":[]}"#.to_string();
        assert!(validate_start_artifact_params(bad).is_err());
    }

    #[test]
    fn test_validate_append_blocks_params_ok() {
        let ok = r#"{"artifactId":"id-1","blocks":[{"type":"paragraph","text":"hi"}],"isComplete":false}"#.to_string();
        assert!(validate_append_blocks_params(ok).is_ok());
    }

    #[test]
    fn test_validate_append_blocks_params_rejects_document_shape() {
        let bad = r#"{"version":"1.0","metadata":{"title":"x"},"blocks":[]}"#.to_string();
        assert!(validate_append_blocks_params(bad).is_err());
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
    fn test_generate_docx_markdown_bold_in_document_xml() {
        let json = r#"{
            "version": "1.0",
            "metadata": { "title": "T", "author": null },
            "blocks": [
                { "type": "paragraph", "text": "Hello **world**" }
            ]
        }"#
        .to_string();
        let bytes = generate_docx(json).expect("docx");
        let cursor = Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("zip");
        let mut f = archive.by_name("word/document.xml").expect("document.xml");
        let mut xml = String::new();
        f.read_to_string(&mut xml).expect("read xml");
        assert!(
            xml.contains("<w:b") || xml.contains("w:b/>"),
            "expected bold run in document.xml: {}",
            &xml[..xml.len().min(2000)]
        );
        assert!(!xml.contains("**"), "markdown markers should not appear as literal text");
    }

    #[test]
    fn test_generate_pdf_extract_has_no_markdown_asterisks() {
        let json = r#"{
            "version": "1.0",
            "metadata": { "title": "T", "author": null },
            "blocks": [
                { "type": "paragraph", "text": "Say **Hi**" }
            ]
        }"#
        .to_string();
        let pdf = generate_pdf(json).expect("pdf");
        let text = extract_text(pdf, "application/pdf".to_string()).expect("extract");
        assert!(!text.contains("**"), "text was: {}", text);
        assert!(text.contains("Hi") || text.contains("hi"), "text was: {}", text);
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

    #[test]
    fn test_generate_docx_heading_fonts() {
        let json = r#"{
            "version": "1.0",
            "metadata": { "title": "T", "author": null },
            "blocks": [
                { "type": "heading", "level": 1, "text": "H1 Title" }
            ]
        }"#.to_string();
        let bytes = generate_docx(json).expect("docx");
        let cursor = Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("zip");
        let mut f = archive.by_name("word/document.xml").expect("document.xml");
        let mut xml = String::new();
        f.read_to_string(&mut xml).expect("read xml");
        
        // Check for Arial font in headings
        assert!(xml.contains("w:ascii=\"Arial\""), "Heading should have Arial font");
        // Check for font size 34 (17pt * 2)
        assert!(xml.contains("<w:sz w:val=\"34\""), "Heading 1 should have size 34");
    }
}

#[cfg(test)]
mod pdf_code_block_layout_tests {
    use super::*;

    // Extract all PDF content-stream operations from page 1 as (operator, Vec<f32>) pairs.
    // Both Integer and Real operands are converted to f32; non-numeric become 0.0.
    fn extract_page1_ops(pdf: Vec<u8>) -> Vec<(String, Vec<f32>)> {
        let doc = lopdf::Document::load_mem(&pdf).expect("load pdf");
        let pages = doc.get_pages();
        let page_id = *pages.get(&1).expect("page 1");
        let content_data = doc.get_page_content(page_id).expect("content");
        let content = lopdf::content::Content::decode(&content_data).expect("decode");
        content.operations.iter().map(|op| {
            let args: Vec<f32> = op.operands.iter().map(|o| {
                o.as_float().ok()
                    .or_else(|| o.as_i64().ok().map(|i| i as f32))
                    .unwrap_or(0.0)
            }).collect();
            (op.operator.clone(), args)
        }).collect()
    }

    // Returns (rect_bottom, rect_height, code_tm_y) triples by pairing each full-width
    // code-background rect with the Tm that immediately follows it in the stream.
    // Full-width means x ≈ MARGIN_L (48) and w > 400 (excludes the 3-pt accent strip).
    fn code_rect_tm_pairs(ops: &[(String, Vec<f32>)]) -> Vec<(f32, f32, f32)> {
        let mut pairs = vec![];
        let mut pending: Option<(f32, f32)> = None; // (rect_bottom, rect_h)
        for (op, args) in ops {
            match op.as_str() {
                "re" if args.len() == 4
                    && (args[0] - 48.0).abs() < 2.0
                    && args[2] > 400.0 =>
                {
                    pending = Some((args[1], args[3]));
                }
                "Tm" if args.len() == 6 => {
                    if let Some((rb, rh)) = pending.take() {
                        pairs.push((rb, rh, args[5]));
                    }
                }
                _ => {}
            }
        }
        pairs
    }

    #[test]
    fn test_code_bg_rect_contains_text_baseline() {
        // Each code line's dark background rect must contain its text baseline (Tm y).
        // Rect top must also clear the Courier 9pt ascender above the baseline (~6.81 pt).
        let json = r#"{
            "version": "1.0",
            "metadata": { "title": "T", "author": null },
            "blocks": [
                { "type": "code", "language": null,
                  "text": "let x = 1;\nlet y = 2;\nlet z = 3;" }
            ]
        }"#.to_string();

        let ops = extract_page1_ops(generate_pdf(json).expect("pdf"));
        let pairs = code_rect_tm_pairs(&ops);

        assert_eq!(pairs.len(), 3, "expected 3 (rect, Tm) pairs for 3 code lines, got {}", pairs.len());

        for (i, &(rect_bottom, rect_h, tm_y)) in pairs.iter().enumerate() {
            let rect_top = rect_bottom + rect_h;
            assert!(
                tm_y > rect_bottom,
                "line {i}: text baseline {tm_y:.2} not above rect bottom {rect_bottom:.2}"
            );
            assert!(
                tm_y < rect_top,
                "line {i}: text baseline {tm_y:.2} not below rect top {rect_top:.2}"
            );
            // Rect top must clear Courier 9pt ascender (≈6.81 pt above baseline).
            assert!(
                rect_top >= tm_y + 6.5,
                "line {i}: rect top {rect_top:.2} < baseline {tm_y:.2} + 6.5 — ascenders clipped"
            );
        }
    }

    #[test]
    fn test_code_bg_rect_extends_below_baseline_by_at_least_descender() {
        // Rect bottom must be below the baseline by at least the Courier 9pt descender (≈2.2 pt).
        let json = r#"{
            "version": "1.0",
            "metadata": { "title": "T", "author": null },
            "blocks": [
                { "type": "code", "language": null, "text": "fn main() {}" }
            ]
        }"#.to_string();

        let ops = extract_page1_ops(generate_pdf(json).expect("pdf"));
        let pairs = code_rect_tm_pairs(&ops);

        assert!(!pairs.is_empty(), "no code rect/Tm pairs found");
        for (i, &(rect_bottom, _, tm_y)) in pairs.iter().enumerate() {
            assert!(
                rect_bottom <= tm_y - 2.0,
                "line {i}: rect bottom {rect_bottom:.2} not ≥ 2 pt below baseline {tm_y:.2}"
            );
        }
    }

    #[test]
    fn test_paragraph_after_code_block_ascenders_clear_rect_bottom() {
        // Times-Roman 12pt ascender ≈ 8.196 pt (683/1000 × 12).
        // The paragraph baseline must sit far enough below the last code rect's bottom
        // that the paragraph's ascenders do not intrude into the dark background.
        const TIMES_ROMAN_12_ASCENDER: f32 = 8.196;

        let json = r#"{
            "version": "1.0",
            "metadata": { "title": "T", "author": null },
            "blocks": [
                { "type": "code", "language": null, "text": "fn foo() {}" },
                { "type": "paragraph", "text": "Paragraph after code." }
            ]
        }"#.to_string();

        let ops = extract_page1_ops(generate_pdf(json).expect("pdf"));
        let pairs = code_rect_tm_pairs(&ops);

        assert!(!pairs.is_empty(), "no code rect/Tm pairs");
        let (last_rect_bottom, _, last_code_tm_y) = *pairs.last().unwrap();

        // Find the first Tm y that is below last_code_tm_y by more than the code line
        // height (≈12.15 pt) — that is the paragraph's baseline.
        let para_tm_y = ops.iter()
            .filter(|(op, args)| op == "Tm" && args.len() == 6)
            .map(|(_, a)| a[5])
            .find(|&y| y < last_code_tm_y - 10.0)
            .expect("paragraph Tm not found");

        let para_ascender_top = para_tm_y + TIMES_ROMAN_12_ASCENDER;
        assert!(
            para_ascender_top <= last_rect_bottom,
            "paragraph ascender top ({para_ascender_top:.2}) overlaps code rect bottom \
             ({last_rect_bottom:.2}) by {:.2} pt — increase vspace after code block",
            para_ascender_top - last_rect_bottom
        );
    }
}
