use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

uniffi::setup_scaffolding!();

mod docx_impl;
mod highlight;
mod latex_to_typst;
mod math_png;
mod md_render;
mod typst_gen;
mod typst_world;

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
    Math {
        display: bool,
        latex: String,
    },
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
    Footnote {
        id: String,
        text: String,
    },
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
    serde_json::from_str::<StartArtifactParams>(&json)
        .map_err(|e| DocGenError::ValidationError(format!("start_artifact arguments: {e}")))?;
    Ok(())
}

#[uniffi::export]
pub fn validate_append_blocks_params(json: String) -> Result<(), DocGenError> {
    serde_json::from_str::<AppendBlocksParams>(&json)
        .map_err(|e| DocGenError::ValidationError(format!("append_blocks arguments: {e}")))?;
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
    let schema_value = serde_json::to_value(&schema_json)
        .map_err(|e| DocGenError::SerializationError(e.to_string()))?;

    let validator = jsonschema::validator_for(&schema_value)
        .map_err(|e| DocGenError::ValidationError(e.to_string()))?;

    let instance: serde_json::Value =
        serde_json::from_str(&json).map_err(|e| DocGenError::SerializationError(e.to_string()))?;

    let error_messages: Vec<String> = validator
        .iter_errors(&instance)
        .map(|e| e.to_string())
        .collect();

    if !error_messages.is_empty() {
        return Err(DocGenError::ValidationError(error_messages.join(", ")));
    }

    // Also try to deserialize to make sure it's fully valid
    let _: Document =
        serde_json::from_str(&json).map_err(|e| DocGenError::SerializationError(e.to_string()))?;

    Ok(())
}

#[uniffi::export]
pub fn generate_docx(json: String) -> Result<Vec<u8>, DocGenError> {
    let doc: Document =
        serde_json::from_str(&json).map_err(|e| DocGenError::SerializationError(e.to_string()))?;
    docx_impl::build_docx_bytes(&doc)
}

#[uniffi::export]
pub fn generate_pdf(json: String) -> Result<Vec<u8>, DocGenError> {
    let doc: Document =
        serde_json::from_str(&json).map_err(|e| DocGenError::SerializationError(e.to_string()))?;
    let generated = typst_gen::json_to_typst(&doc);
    typst_gen::compile_to_pdf(generated)
}

#[uniffi::export]
pub fn extract_text(bytes: Vec<u8>, mime_type: String) -> Result<String, DocGenError> {
    match mime_type.as_str() {
        "application/pdf" => pdf_extract::extract_text_from_mem(&bytes)
            .map_err(|e| DocGenError::ExtractionError(e.to_string())),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
            Ok("DOCX extraction not fully implemented".to_string())
        }
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => {
            use calamine::{Data, Reader, Xlsx};
            let cursor = Cursor::new(bytes);
            let mut excel: Xlsx<_> = calamine::open_workbook_from_rs(cursor)
                .map_err(|e: calamine::XlsxError| DocGenError::ExtractionError(e.to_string()))?;
            let mut text = String::new();
            if let Some(Ok(range)) = excel.worksheet_range_at(0) {
                for row in range.rows() {
                    for cell in row {
                        match cell {
                            Data::Empty => {}
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

    struct DocxParts {
        document_xml: String,
        rels_xml: String,
        media: Vec<(String, Vec<u8>)>,
    }

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

    fn math_docx_json(display: bool, latex_json_value: &str) -> String {
        format!(
            r#"{{
                "version": "1.0",
                "metadata": {{ "title": "T", "author": null }},
                "blocks": [
                    {{ "type": "math", "display": {display}, "latex": "{latex_json_value}" }}
                ]
            }}"#,
        )
    }

    fn inspect_docx(bytes: Vec<u8>) -> DocxParts {
        let cursor = Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("zip");
        let mut document_xml = String::new();
        archive
            .by_name("word/document.xml")
            .expect("document.xml")
            .read_to_string(&mut document_xml)
            .expect("read document.xml");

        let mut rels_xml = String::new();
        archive
            .by_name("word/_rels/document.xml.rels")
            .expect("document.xml.rels")
            .read_to_string(&mut rels_xml)
            .expect("read document.xml.rels");

        let mut media = Vec::new();
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).expect("zip entry");
            let name = file.name().to_string();
            if name.starts_with("word/media/image") && name.ends_with(".png") {
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes).expect("read media");
                media.push((name, bytes));
            }
        }

        DocxParts {
            document_xml,
            rels_xml,
            media,
        }
    }

    fn docx_text_from_w_t(document_xml: &str) -> String {
        let mut text = String::new();
        let mut rest = document_xml;
        while let Some(start) = rest.find("<w:t") {
            rest = &rest[start..];
            let Some(close) = rest.find('>') else {
                break;
            };
            rest = &rest[close + 1..];
            let Some(end) = rest.find("</w:t>") else {
                break;
            };
            text.push_str(&rest[..end]);
            rest = &rest[end + "</w:t>".len()..];
        }
        text
    }

    fn first_media_dimensions(parts: &DocxParts) -> (u32, u32) {
        let (_, bytes) = parts.media.first().expect("png media");
        let image = image::load_from_memory(bytes).expect("decode png");
        image::GenericImageView::dimensions(&image)
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
        assert!(
            validate_start_artifact_params(r#"{"title":"T","author":"A"}"#.to_string()).is_ok()
        );
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
        }"#
        .to_string();
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
        }"#
        .to_string();
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
        assert!(
            !xml.contains("**"),
            "markdown markers should not appear as literal text"
        );
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
        assert!(
            text.contains("Hi") || text.contains("hi"),
            "text was: {}",
            text
        );
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
    fn test_generate_docx_display_math_embeds_png_caption_and_centering() {
        let bytes = generate_docx(math_docx_json(true, "E = mc^2")).expect("docx");
        let parts = inspect_docx(bytes);

        assert!(!parts.document_xml.contains("MATHBLOCK_0"));
        assert!(!parts.document_xml.contains("<m:oMath"));
        assert_eq!(parts.media.len(), 1, "expected one PNG media entry");
        assert!(parts.document_xml.contains("LaTeX source: E = mc^2"));
        assert!(parts.document_xml.contains(r#"w:jc w:val="center""#));
    }

    #[test]
    fn test_generate_docx_inline_math_embeds_png_and_caption() {
        let bytes = generate_docx(math_docx_json(false, "x^2")).expect("docx");
        let parts = inspect_docx(bytes);

        assert_eq!(parts.media.len(), 1, "expected one PNG media entry");
        assert!(parts.document_xml.contains("LaTeX source: x^2"));
    }

    #[test]
    fn test_generate_docx_math_has_no_legacy_placeholder_or_omml() {
        let bytes = generate_docx(math_docx_json(true, "E = mc^2")).expect("docx");
        let parts = inspect_docx(bytes);

        assert!(!parts.document_xml.contains("MATHBLOCK_"));
        assert!(!parts.document_xml.contains("<m:oMath"));
    }

    #[test]
    fn test_generate_docx_math_caption_preserves_two_consecutive_backslashes() {
        let bytes = generate_docx(math_docx_json(true, r#"a\\\\b"#)).expect("docx");
        let parts = inspect_docx(bytes);
        let text = docx_text_from_w_t(&parts.document_xml);

        assert!(text.contains(r"LaTeX source: a\\b"), "text was: {text}");
    }

    #[test]
    fn test_generate_docx_math_image_relationship_references_existing_png() {
        let bytes = generate_docx(math_docx_json(true, "E = mc^2")).expect("docx");
        let parts = inspect_docx(bytes);
        let embed_pos = parts
            .document_xml
            .find("r:embed=\"")
            .expect("image embed relationship");
        let id_start = embed_pos + "r:embed=\"".len();
        let id_end = parts.document_xml[id_start..].find('"').expect("rId end") + id_start;
        let rid = &parts.document_xml[id_start..id_end];

        assert!(parts.rels_xml.contains(&format!(r#"Id="{rid}""#)));
        assert!(parts.rels_xml.contains("/image"));
        assert!(parts.rels_xml.contains("media/image"));
        assert_eq!(parts.media.len(), 1, "expected one PNG media entry");
    }

    #[test]
    fn test_generate_docx_invalid_math_keeps_caption_and_failure_marker() {
        let bytes = generate_docx(math_docx_json(
            true,
            crate::math_png::INVALID_MATH_LATEX_FOR_DOCX
                .replace('\\', r"\\")
                .as_str(),
        ))
        .expect("docx");
        let parts = inspect_docx(bytes);

        assert!(parts.document_xml.contains("[Equation render failed]"));
        assert!(parts.document_xml.contains(&format!(
            "LaTeX source: {}",
            crate::math_png::INVALID_MATH_LATEX_FOR_DOCX
        )));
    }

    #[test]
    fn test_generate_docx_empty_math_keeps_caption_and_failure_marker() {
        let bytes = generate_docx(math_docx_json(true, "")).expect("docx");
        let parts = inspect_docx(bytes);

        assert!(parts.document_xml.contains("[Equation render failed]"));
        assert!(parts.document_xml.contains("LaTeX source: "));
    }

    #[test]
    fn test_generate_docx_fraction_math_png_has_real_render_dimensions() {
        let bytes = generate_docx(math_docx_json(true, r#"\\frac{1}{2}"#)).expect("docx");
        let parts = inspect_docx(bytes);
        let (width, height) = first_media_dimensions(&parts);

        assert!(width >= 40, "width was {width}");
        assert!(height >= 20, "height was {height}");
    }

    #[test]
    fn test_generate_docx_same_math_has_stable_png_dimensions() {
        let first = inspect_docx(
            generate_docx(math_docx_json(true, r#"\\frac{1}{2}"#)).expect("first docx"),
        );
        let second = inspect_docx(
            generate_docx(math_docx_json(true, r#"\\frac{1}{2}"#)).expect("second docx"),
        );

        assert_eq!(
            first_media_dimensions(&first),
            first_media_dimensions(&second)
        );
    }

    #[test]
    fn test_generate_docx_paragraph_inline_dollar_not_literal_delimiters() {
        let json = r#"{
            "version": "1.0",
            "metadata": { "title": "T", "author": null },
            "blocks": [
                { "type": "paragraph", "text": "Einstein $E = mc^2$ ties." }
            ]
        }"#
        .to_string();
        let bytes = generate_docx(json).expect("docx");
        let cursor = Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("zip");
        let mut xml = String::new();
        archive
            .by_name("word/document.xml")
            .expect("document.xml")
            .read_to_string(&mut xml)
            .expect("read xml");
        assert!(
            !xml.contains("$E = mc^2$"),
            "dollar-delimited math should not appear literally; xml snippet: {}",
            &xml[xml.find("Einstein").unwrap_or(0)
                ..(xml.find("Einstein").unwrap_or(0) + 400).min(xml.len())]
        );
        assert!(xml.contains("E = mc^2"));
    }

    #[test]
    fn test_generate_docx_heading_fonts() {
        let json = r#"{
            "version": "1.0",
            "metadata": { "title": "T", "author": null },
            "blocks": [
                { "type": "heading", "level": 1, "text": "H1 Title" }
            ]
        }"#
        .to_string();
        let bytes = generate_docx(json).expect("docx");
        let cursor = Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("zip");
        let mut f = archive.by_name("word/document.xml").expect("document.xml");
        let mut xml = String::new();
        f.read_to_string(&mut xml).expect("read xml");

        // Check for Arial font in headings
        assert!(
            xml.contains("w:ascii=\"Arial\""),
            "Heading should have Arial font"
        );
        // Check for font size 34 (17pt * 2)
        assert!(
            xml.contains("<w:sz w:val=\"34\""),
            "Heading 1 should have size 34"
        );
    }
}

#[cfg(test)]
mod typst_pdf_tests {
    use super::*;

    // ── S1b — Emoji in paragraph does not emit missing-glyph placeholder ────

    #[test]
    fn test_typst_pdf_emoji_renders_without_noglyph_placeholder() {
        let json = r#"{
            "version": "1.0",
            "metadata": { "title": "Emoji", "author": null },
            "blocks": [
                { "type": "paragraph", "text": "Hello 👋 world 🎉" }
            ]
        }"#
        .to_string();

        let bytes = generate_pdf(json).expect("generate_pdf with emoji failed");
        assert_eq!(&bytes[0..4], b"%PDF");
        assert!(
            !bytes
                .windows(7)
                .any(|window| window.eq_ignore_ascii_case(b"NOGLYPH")),
            "PDF still contains NOGLYPH missing-glyph placeholder"
        );
    }

    // ── S1 — PDF header bytes ─────────────────────────────────────────────────

    #[test]
    fn test_typst_pdf_starts_with_percent_pdf() {
        let json = r#"{
            "version": "1.0",
            "metadata": { "title": "Hello", "author": null },
            "blocks": [
                { "type": "paragraph", "text": "Hello world." }
            ]
        }"#
        .to_string();

        let bytes = generate_pdf(json).expect("generate_pdf failed");

        assert!(!bytes.is_empty(), "PDF bytes must not be empty");
        assert_eq!(
            &bytes[0..4],
            b"%PDF",
            "PDF must start with %PDF, got: {:?}",
            &bytes[..4.min(bytes.len())]
        );
    }

    // ── S2 — All Block types produce no error ─────────────────────────────────

    #[test]
    fn test_typst_pdf_all_block_types_no_error() {
        let doc = Document {
            version: "1.0".to_string(),
            metadata: Metadata { title: "All Blocks".to_string(), author: Some("Tester".to_string()) },
            blocks: vec![
                Block::Heading { level: 1, text: "Title".to_string(), anchor: Some("top".to_string()) },
                Block::Heading { level: 2, text: "Section".to_string(), anchor: None },
                Block::Heading { level: 3, text: "Subsection".to_string(), anchor: None },
                Block::Paragraph { text: "Plain text with **bold** and *italic*.".to_string(), align: None },
                Block::Paragraph { text: "Centered.".to_string(), align: Some(Alignment::Center) },
                Block::Code { language: Some("rust".to_string()), text: "fn main() {}".to_string(),
                              show_line_numbers: false, caption: None, highlight_lines: vec![] },
                Block::Code { language: None, text: "plain code".to_string(),
                              show_line_numbers: true, caption: Some("Listing 1".to_string()),
                              highlight_lines: vec![] },
                Block::List { ordered: false, items: vec![
                    ListItem { text: "Alpha".to_string(), checked: None, children: vec![
                        ListItem { text: "Nested".to_string(), checked: None, children: vec![] }
                    ]},
                    ListItem { text: "Beta".to_string(), checked: Some(true), children: vec![] },
                ], start: None },
                Block::List { ordered: true, items: vec![
                    ListItem { text: "One".to_string(), checked: None, children: vec![] },
                    ListItem { text: "Two".to_string(), checked: None, children: vec![] },
                ], start: Some(5) },
                Block::Table {
                    header: vec!["Col A".to_string(), "Col B".to_string()],
                    rows: vec![vec!["R1A".to_string(), "R1B".to_string()]],
                    alignments: Some(vec![Alignment::Left, Alignment::Right]),
                    caption: Some("Sample table".to_string()),
                },
                Block::Math { display: true, latex: "E = mc^2".to_string() },
                // 1x1 transparent PNG as base64
                Block::Image {
                    src: "tiny.png".to_string(),
                    alt: Some("Tiny image".to_string()),
                    data_base64: Some(
                        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=".to_string()
                    ),
                    mime: Some("image/png".to_string()),
                    width_pt: Some(10.0),
                    height_pt: Some(10.0),
                    align: None,
                },
                Block::Blockquote { text: "To be or not to be.".to_string(),
                                    attribution: Some("Shakespeare".to_string()) },
                Block::HorizontalRule,
                Block::Callout { kind: CalloutKind::Warning, title: Some("Heads up".to_string()),
                                 text: "Be careful.".to_string() },
                Block::Callout { kind: CalloutKind::Note, title: None,
                                 text: "Just a note.".to_string() },
                Block::Footnote { id: "fn1".to_string(), text: "Footnote text here.".to_string() },
                Block::Toc { max_depth: Some(2) },
                Block::PageBreak,
                Block::Paragraph { text: "After page break.".to_string(), align: None },
            ],
        };
        let json = serde_json::to_string(&doc).unwrap();
        let bytes =
            generate_pdf(json).expect("generate_pdf must not error for all-blocks document");
        assert_eq!(&bytes[0..4], b"%PDF");
    }

    // ── S3 — Text extraction recovers heading ──────────────────────────────────

    #[test]
    fn test_typst_pdf_extract_text_contains_heading() {
        let json = r#"{
            "version": "1.0",
            "metadata": { "title": "Extraction Test", "author": null },
            "blocks": [
                { "type": "heading", "level": 1, "text": "Project Phoenix" },
                { "type": "paragraph", "text": "Short body." }
            ]
        }"#
        .to_string();

        let pdf_bytes = generate_pdf(json).expect("pdf");
        let text = extract_text(pdf_bytes, "application/pdf".to_string()).expect("extract");

        assert!(
            text.contains("Project Phoenix"),
            "expected 'Project Phoenix' in extracted text, got: {}",
            &text[..text.len().min(500)]
        );
    }

    // ── S4 — No markdown asterisk literals in extracted text ──────────────────

    #[test]
    fn test_typst_pdf_no_markdown_asterisks_in_extracted_text() {
        let json = r#"{
            "version": "1.0",
            "metadata": { "title": "T", "author": null },
            "blocks": [
                { "type": "paragraph", "text": "Say **Hi** there." }
            ]
        }"#
        .to_string();

        let pdf_bytes = generate_pdf(json).expect("pdf");
        let text = extract_text(pdf_bytes, "application/pdf".to_string()).expect("extract");

        assert!(
            !text.contains("**"),
            "literal ** appeared in extracted text: {}",
            text
        );
        assert!(
            text.contains("Hi"),
            "word 'Hi' missing from extracted text: {}",
            &text[..text.len().min(500)]
        );
    }

    // ── M1 — Automatic pagination ─────────────────────────────────────────────

    #[test]
    fn test_typst_pdf_multi_page_long_content() {
        let mut blocks = Vec::new();
        for i in 0..90 {
            blocks.push(Block::Paragraph {
                text: format!(
                    "Paragraph {}. Lorem ipsum dolor sit amet, consectetur adipiscing elit, \
                     sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.",
                    i
                ),
                align: None,
            });
        }
        let doc = Document {
            version: "1.0".to_string(),
            metadata: Metadata {
                title: "Pagination Test".to_string(),
                author: None,
            },
            blocks,
        };
        let json = serde_json::to_string(&doc).unwrap();
        let pdf_bytes = generate_pdf(json).expect("pdf");

        let lopdf_doc = lopdf::Document::load_mem(&pdf_bytes).expect("lopdf");
        let page_count = lopdf_doc.get_pages().len();
        assert!(
            page_count >= 2,
            "expected >= 2 pages for 90 paragraphs, got {}",
            page_count
        );
    }

    // ── M2 — Explicit PageBreak ────────────────────────────────────────────────

    #[test]
    fn test_typst_pdf_explicit_page_break_produces_two_pages() {
        let doc = Document {
            version: "1.0".to_string(),
            metadata: Metadata {
                title: "Break Test".to_string(),
                author: None,
            },
            blocks: vec![
                Block::Paragraph {
                    text: "Page one.".to_string(),
                    align: None,
                },
                Block::PageBreak,
                Block::Paragraph {
                    text: "Page two.".to_string(),
                    align: None,
                },
            ],
        };
        let json = serde_json::to_string(&doc).unwrap();
        let pdf_bytes = generate_pdf(json).expect("pdf");
        let lopdf_doc = lopdf::Document::load_mem(&pdf_bytes).expect("lopdf");
        assert_eq!(lopdf_doc.get_pages().len(), 2);
    }

    // ── E1 — Invalid JSON returns SerializationError ──────────────────────────

    #[test]
    fn test_typst_pdf_invalid_json_returns_serialization_error() {
        let result = generate_pdf("not valid json".to_string());
        assert!(result.is_err());
        match result.unwrap_err() {
            DocGenError::SerializationError(_) => {}
            other => panic!("expected SerializationError, got {:?}", other),
        }
    }

    // ── E2 — Empty blocks list generates valid PDF ────────────────────────────

    #[test]
    fn test_typst_pdf_empty_blocks_produces_valid_pdf() {
        let json = r#"{
            "version": "1.0",
            "metadata": { "title": "Empty", "author": null },
            "blocks": []
        }"#
        .to_string();
        let bytes = generate_pdf(json).expect("empty-blocks PDF must succeed");
        assert_eq!(&bytes[0..4], b"%PDF");
    }

    // ── E3 — Deeply nested list compiles (stack-size sanity) ──────────────────

    #[test]
    fn test_typst_pdf_deeply_nested_list_no_stack_overflow() {
        // Build a 40-level deep nested unordered list.
        fn nest(depth: u32) -> ListItem {
            ListItem {
                text: format!("Level {}", depth),
                checked: None,
                children: if depth == 0 {
                    vec![]
                } else {
                    vec![nest(depth - 1)]
                },
            }
        }
        let doc = Document {
            version: "1.0".to_string(),
            metadata: Metadata {
                title: "Nested".to_string(),
                author: None,
            },
            blocks: vec![Block::List {
                ordered: false,
                items: vec![nest(40)],
                start: None,
            }],
        };
        let json = serde_json::to_string(&doc).unwrap();
        let bytes = generate_pdf(json).expect("deep nesting must not stack-overflow");
        assert_eq!(&bytes[0..4], b"%PDF");
    }

    // ── E4 — LaTeX-only math does not crash the pipeline ─────────────────────

    #[test]
    fn test_typst_pdf_latex_only_math_falls_back_cleanly() {
        let doc = Document {
            version: "1.0".to_string(),
            metadata: Metadata {
                title: "Math".to_string(),
                author: None,
            },
            blocks: vec![
                Block::Paragraph {
                    text: "Before math.".to_string(),
                    align: None,
                },
                Block::Math {
                    display: true,
                    latex: r"\sum_{i=0}^{n} i^2".to_string(),
                },
                Block::Paragraph {
                    text: "After math.".to_string(),
                    align: None,
                },
            ],
        };
        let json = serde_json::to_string(&doc).unwrap();
        // Generator detects backslash-alpha → emits comment + raw fallback.
        // Pipeline must succeed (no GenerationError).
        let bytes = generate_pdf(json).expect("LaTeX fallback must produce valid PDF");
        assert_eq!(&bytes[0..4], b"%PDF");
    }
}
