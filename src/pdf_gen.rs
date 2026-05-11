//! PDF generation using `lopdf` (replaces `printpdf` for `generate_pdf`).

use crate::md_render::{markdown_inline_to_spans, wrap_spans_to_lines, StyledSpan};
use crate::{Block, DocGenError, Document};
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document as LopdfDocument, Object, Stream};

const PAGE_W: f32 = 595.0;
const PAGE_H: f32 = 842.0;
const MARGIN_L: f32 = 50.0;
const LINE_H: f32 = 14.0;
const MAX_LINE_CHARS: usize = 85;

fn escape_pdf_literal(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

fn text_op(s: &str) -> Operation {
    Operation::new(
        "Tj",
        vec![Object::string_literal(escape_pdf_literal(s))],
    )
}

fn push_line(ops: &mut Vec<Operation>, first_position: &mut bool, _yy: f32, chunks: &[(String, f32, bool)]) {
    if !chunks.iter().any(|c| !c.0.is_empty()) {
        return;
    }
    if *first_position {
        ops.push(Operation::new(
            "Td",
            vec![MARGIN_L.into(), (PAGE_H - 72.0).into()],
        ));
        *first_position = false;
    } else {
        ops.push(Operation::new("Td", vec![0.into(), (-LINE_H).into()]));
    }
    for (text, size, bold) in chunks {
        if text.is_empty() {
            continue;
        }
        let font = if *bold { "F2" } else { "F1" };
        ops.push(Operation::new(
            "Tf",
            vec![font.into(), (*size).into()],
        ));
        ops.push(text_op(text));
    }
}

fn line_chunks_from_spans(spans: &[StyledSpan], base_size: f32) -> Vec<Vec<(String, f32, bool)>> {
    let mut lines_out: Vec<Vec<(String, f32, bool)>> = vec![];
    for line in wrap_spans_to_lines(spans, MAX_LINE_CHARS) {
        let mut row: Vec<(String, f32, bool)> = vec![];
        for seg in &line {
            let t = seg.text.replace('\n', " ");
            if t.is_empty() {
                continue;
            }
            row.push((t, base_size, seg.bold));
        }
        if !row.is_empty() {
            lines_out.push(row);
        }
    }
    if lines_out.is_empty() {
        lines_out.push(vec![]);
    }
    lines_out
}

pub fn generate_pdf_from_document(doc: &Document) -> Result<Vec<u8>, DocGenError> {
    let mut pdf = LopdfDocument::with_version("1.5");
    let pages_id = pdf.new_object_id();

    let font_regular = pdf.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let font_bold = pdf.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica-Bold",
    });

    let resources_id = pdf.add_object(dictionary! {
        "Font" => dictionary! {
            "F1" => font_regular,
            "F2" => font_bold,
        },
    });

    let mut ops: Vec<Operation> = vec![Operation::new("BT", vec![])];
    let mut first_pos = true;

    push_line(
        &mut ops,
        &mut first_pos,
        PAGE_H - 72.0,
        &[(doc.metadata.title.clone(), 18.0, true)],
    );

    if let Some(author) = &doc.metadata.author {
        push_line(
            &mut ops,
            &mut first_pos,
            0.0,
            &[(format!("By {}", author), 11.0, false)],
        );
    }

    for block in &doc.blocks {
        match block {
            Block::Heading { level, text } => {
                let size = match level {
                    1 => 16.0,
                    2 => 14.0,
                    _ => 12.0,
                };
                let spans = markdown_inline_to_spans(text);
                for row in line_chunks_from_spans(&spans, size) {
                    if row.is_empty() {
                        continue;
                    }
                    push_line(&mut ops, &mut first_pos, 0.0, &row);
                }
            }
            Block::Paragraph { text } => {
                let spans = markdown_inline_to_spans(text);
                for row in line_chunks_from_spans(&spans, 11.0) {
                    if row.is_empty() {
                        continue;
                    }
                    push_line(&mut ops, &mut first_pos, 0.0, &row);
                }
            }
            Block::List { ordered, items } => {
                for (i, item) in items.iter().enumerate() {
                    let prefix = if *ordered {
                        format!("{}. ", i + 1)
                    } else {
                        "• ".to_string()
                    };
                    let spans = markdown_inline_to_spans(item);
                    let lines = wrap_spans_to_lines(&spans, MAX_LINE_CHARS.saturating_sub(4));
                    let mut is_first = true;
                    for line in lines {
                        let mut row: Vec<(String, f32, bool)> = vec![];
                        if is_first {
                            row.push((prefix.clone(), 11.0, false));
                            is_first = false;
                        }
                        for seg in &line {
                            let t = seg.text.replace('\n', " ");
                            if !t.is_empty() {
                                row.push((t, 11.0, seg.bold));
                            }
                        }
                        if row.is_empty() {
                            continue;
                        }
                        push_line(&mut ops, &mut first_pos, 0.0, &row);
                    }
                }
            }
            Block::Table { header, rows, .. } => {
                for h in header {
                    push_line(
                        &mut ops,
                        &mut first_pos,
                        0.0,
                        &[(h.replace('\n', " "), 10.0, true)],
                    );
                }
                for row in rows {
                    let row_s = row.join(" | ");
                    push_line(
                        &mut ops,
                        &mut first_pos,
                        0.0,
                        &[(row_s, 9.0, false)],
                    );
                }
            }
            Block::Code { text, .. } => {
                for line in text.lines() {
                    push_line(
                        &mut ops,
                        &mut first_pos,
                        0.0,
                        &[(line.to_string(), 9.0, false)],
                    );
                }
            }
            Block::Math { latex, .. } => {
                push_line(
                    &mut ops,
                    &mut first_pos,
                    0.0,
                    &[(latex.replace('\n', " "), 10.0, false)],
                );
            }
            Block::Image { src, alt } => {
                let alt_text = alt.clone().unwrap_or_else(|| "Image".to_string());
                let s = format!("[{}] {}", alt_text, src);
                push_line(
                    &mut ops,
                    &mut first_pos,
                    0.0,
                    &[(s, 10.0, false)],
                );
            }
            Block::PageBreak => {
                if !first_pos {
                    ops.push(Operation::new("Td", vec![0.into(), (-24.0).into()]));
                }
            }
        }
    }
    ops.push(Operation::new("ET", vec![]));

    let content = Content { operations: ops };
    let content_id = pdf.add_object(Stream::new(
        dictionary! {},
        content
            .encode()
            .map_err(|e| DocGenError::GenerationError(e.to_string()))?,
    ));

    let page_id = pdf.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
    });

    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), PAGE_W.into(), PAGE_H.into()],
    };
    pdf.objects.insert(pages_id, Object::Dictionary(pages));

    let catalog_id = pdf.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    pdf.trailer.set("Root", catalog_id);
    pdf.compress();

    let mut buf = Vec::new();
    pdf.save_to(&mut buf)
        .map_err(|e| DocGenError::GenerationError(e.to_string()))?;
    Ok(buf)
}
