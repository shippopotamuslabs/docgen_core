//! PDF generation using `lopdf` (replaces `printpdf` for `generate_pdf`).

use crate::md_render::{
    markdown_inline_to_spans, max_chars_for_pdf_line, normalize_for_pdf_builtin_fonts,
    wrap_spans_to_lines, PDF_CHAR_WIDTH_EM, StyledSpan,
};
use crate::{Block, DocGenError, Document};
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document as LopdfDocument, Object, Stream};

const PAGE_W: f32 = 595.0;
const PAGE_H: f32 = 842.0;
const MARGIN_L: f32 = 48.0;
const MARGIN_R: f32 = 48.0;
const MARGIN_TOP: f32 = 72.0;
const MARGIN_BOTTOM: f32 = 56.0;

const BODY_PT: f32 = 12.0;
const AUTHOR_PT: f32 = 11.0;
const TITLE_PT: f32 = 20.0;

fn usable_width_pt() -> f32 {
    (PAGE_W - MARGIN_L - MARGIN_R).max(72.0)
}

fn escape_pdf_literal(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

fn text_op(s: &str) -> Operation {
    let normalized = normalize_for_pdf_builtin_fonts(s);
    Operation::new(
        "Tj",
        vec![Object::string_literal(escape_pdf_literal(&normalized))],
    )
}

/// One text line: absolute text matrix then Tf/Tj runs (position advances horizontally).
fn emit_line_tm(ops: &mut Vec<Operation>, x: f32, y: f32, chunks: &[(String, f32, bool)]) {
    if !chunks.iter().any(|c| !c.0.is_empty()) {
        return;
    }
    ops.push(Operation::new(
        "Tm",
        vec![
            1.into(),
            0.into(),
            0.into(),
            1.into(),
            x.into(),
            y.into(),
        ],
    ));
    for (text, size, bold) in chunks {
        if text.is_empty() {
            continue;
        }
        let font = if *bold { "F2" } else { "F1" };
        ops.push(Operation::new("Tf", vec![font.into(), (*size).into()]));
        ops.push(text_op(text));
    }
}

fn line_height_for_font(pt: f32) -> f32 {
    (pt * 1.35).max(12.0)
}

struct PdfTextCanvas {
    pages: Vec<Vec<Operation>>,
    ops: Vec<Operation>,
    /// Baseline Y for the next line (PDF user space, origin bottom-left).
    y: f32,
}

impl PdfTextCanvas {
    fn new() -> Self {
        Self {
            pages: Vec::new(),
            ops: vec![Operation::new("BT", vec![])],
            y: PAGE_H - MARGIN_TOP,
        }
    }

    fn flush_page(&mut self) {
        self.ops.push(Operation::new("ET", vec![]));
        self.pages.push(std::mem::replace(&mut self.ops, vec![Operation::new("BT", vec![])]));
        self.y = PAGE_H - MARGIN_TOP;
    }

    /// Ensure at least `need` points of vertical space remain above the bottom margin.
    fn ensure_vertical_room(&mut self, need: f32) {
        if self.y - need < MARGIN_BOTTOM {
            self.flush_page();
        }
    }

    fn emit_line(&mut self, chunks: &[(String, f32, bool)], font_pt: f32) {
        if !chunks.iter().any(|c| !c.0.is_empty()) {
            return;
        }
        let lh = line_height_for_font(font_pt);
        self.ensure_vertical_room(lh);
        emit_line_tm(&mut self.ops, MARGIN_L, self.y, chunks);
        self.y -= lh;
    }

    /// Extra vertical gap (e.g. after a paragraph or before a heading).
    fn vspace(&mut self, gap_pt: f32) {
        if gap_pt <= 0.0 {
            return;
        }
        self.ensure_vertical_room(gap_pt);
        self.y -= gap_pt;
    }

    fn page_break(&mut self) {
        if self.ops.len() == 1 && self.ops[0].operator == "BT" {
            return;
        }
        self.flush_page();
    }

    fn finish(mut self) -> Vec<Vec<Operation>> {
        self.ops.push(Operation::new("ET", vec![]));
        self.pages.push(self.ops);
        self.pages
    }
}

fn line_chunks_from_spans(spans: &[StyledSpan], base_size: f32) -> Vec<Vec<(String, f32, bool)>> {
    let max_chars = max_chars_for_pdf_line(base_size, usable_width_pt());
    let mut lines_out: Vec<Vec<(String, f32, bool)>> = vec![];
    for line in wrap_spans_to_lines(spans, max_chars) {
        let mut row: Vec<(String, f32, bool)> = vec![];
        for seg in &line {
            let t = normalize_for_pdf_builtin_fonts(&seg.text.replace('\n', " "));
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

fn push_long_unbroken(lines: &mut Vec<String>, token: &str, max_chars: usize) {
    let mut t = token.to_string();
    while !t.is_empty() {
        let take = max_chars.max(1).min(t.chars().count());
        let line: String = t.chars().take(take).collect();
        t = t.chars().skip(take).collect();
        lines.push(line);
    }
}

fn wrap_plain_to_lines(s: &str, font_pt: f32) -> Vec<String> {
    let normalized = normalize_for_pdf_builtin_fonts(s);
    let max_chars = max_chars_for_pdf_line(font_pt, usable_width_pt());
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();

    for w in normalized.split_whitespace() {
        let wlen = w.chars().count();
        if wlen > max_chars {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            push_long_unbroken(&mut lines, w, max_chars);
            continue;
        }
        if current.is_empty() {
            current.push_str(w);
            continue;
        }
        if current.chars().count() + 1 + wlen <= max_chars {
            current.push(' ');
            current.push_str(w);
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(w);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        let flat: String = normalized.chars().filter(|c| !c.is_control()).collect();
        if !flat.is_empty() {
            push_long_unbroken(&mut lines, &flat, max_chars);
        }
    }
    lines
}

pub fn generate_pdf_from_document(doc: &Document) -> Result<Vec<u8>, DocGenError> {
    let mut pdf = LopdfDocument::with_version("1.5");
    let pages_id = pdf.new_object_id();

    let font_regular = pdf.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Times-Roman",
    });
    let font_bold = pdf.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Times-Bold",
    });

    let resources_id = pdf.add_object(dictionary! {
        "Font" => dictionary! {
            "F1" => font_regular,
            "F2" => font_bold,
        },
    });

    let mut canvas = PdfTextCanvas::new();

    let title_pdf = normalize_for_pdf_builtin_fonts(&doc.metadata.title);
    canvas.emit_line(&[(title_pdf, TITLE_PT, true)], TITLE_PT);
    canvas.vspace(8.0);

    if let Some(author) = &doc.metadata.author {
        let line = format!("By {}", normalize_for_pdf_builtin_fonts(author));
        canvas.emit_line(&[(line, AUTHOR_PT, false)], AUTHOR_PT);
        canvas.vspace(14.0);
    } else {
        canvas.vspace(10.0);
    }

    for block in &doc.blocks {
        match block {
            Block::Heading { level, text } => {
                let size = match level {
                    1 => 17.0,
                    2 => 15.0,
                    3 => 13.5,
                    _ => 12.5,
                };
                canvas.vspace(match level {
                    1 => 16.0,
                    2 => 14.0,
                    _ => 12.0,
                });
                let spans = markdown_inline_to_spans(&normalize_for_pdf_builtin_fonts(text));
                for row in line_chunks_from_spans(&spans, size) {
                    if row.is_empty() {
                        continue;
                    }
                    canvas.emit_line(&row, size);
                }
                canvas.vspace(match level {
                    1 => 10.0,
                    2 => 8.0,
                    _ => 6.0,
                });
            }
            Block::Paragraph { text } => {
                let spans = markdown_inline_to_spans(&normalize_for_pdf_builtin_fonts(text));
                for row in line_chunks_from_spans(&spans, BODY_PT) {
                    if row.is_empty() {
                        continue;
                    }
                    canvas.emit_line(&row, BODY_PT);
                }
                canvas.vspace(8.0);
            }
            Block::List { ordered, items } => {
                const PREFIX_CHARS_EST: f32 = 6.0;
                for (i, item) in items.iter().enumerate() {
                    let prefix = if *ordered {
                        format!("{}. ", i + 1)
                    } else {
                        "- ".to_string()
                    };
                    let spans = markdown_inline_to_spans(&normalize_for_pdf_builtin_fonts(item));
                    let list_width =
                        usable_width_pt() - (BODY_PT * PDF_CHAR_WIDTH_EM * PREFIX_CHARS_EST);
                    let max_chars = max_chars_for_pdf_line(BODY_PT, list_width.max(48.0));
                    let lines = wrap_spans_to_lines(&spans, max_chars);
                    let mut is_first = true;
                    for line in lines {
                        let mut row: Vec<(String, f32, bool)> = vec![];
                        if is_first {
                            row.push((prefix.clone(), BODY_PT, false));
                            is_first = false;
                        }
                        for seg in &line {
                            let t = normalize_for_pdf_builtin_fonts(&seg.text.replace('\n', " "));
                            if !t.is_empty() {
                                row.push((t, BODY_PT, seg.bold));
                            }
                        }
                        if row.is_empty() {
                            continue;
                        }
                        canvas.emit_line(&row, BODY_PT);
                    }
                }
                canvas.vspace(6.0);
            }
            Block::Table { header, rows, .. } => {
                canvas.vspace(4.0);
                for h in header {
                    let wrapped = wrap_plain_to_lines(&h.replace('\n', " "), 10.0);
                    for line in wrapped {
                        canvas.emit_line(&[(line, 10.0, true)], 10.0);
                    }
                }
                for row in rows {
                    let row_s = row.join(" | ");
                    let wrapped = wrap_plain_to_lines(&row_s.replace('\n', " "), 9.0);
                    for line in wrapped {
                        canvas.emit_line(&[(line, 9.0, false)], 9.0);
                    }
                }
                canvas.vspace(8.0);
            }
            Block::Code { text, .. } => {
                canvas.vspace(4.0);
                for line in text.lines() {
                    let wrapped = wrap_plain_to_lines(line, 9.0);
                    if wrapped.is_empty() {
                        canvas.vspace(line_height_for_font(9.0));
                    } else {
                        for w in wrapped {
                            canvas.emit_line(&[(w, 9.0, false)], 9.0);
                        }
                    }
                }
                canvas.vspace(8.0);
            }
            Block::Math { latex, .. } => {
                let wrapped = wrap_plain_to_lines(&latex.replace('\n', " "), 10.0);
                for line in wrapped {
                    canvas.emit_line(&[(line, 10.0, false)], 10.0);
                }
                canvas.vspace(6.0);
            }
            Block::Image { src, alt } => {
                let alt_text = alt.clone().unwrap_or_else(|| "Image".to_string());
                let s = format!("[{}] {}", alt_text, src);
                let wrapped = wrap_plain_to_lines(&s, 10.0);
                for line in wrapped {
                    canvas.emit_line(&[(line, 10.0, false)], 10.0);
                }
                canvas.vspace(6.0);
            }
            Block::PageBreak => {
                canvas.page_break();
            }
        }
    }

    let page_streams: Vec<Vec<Operation>> = canvas.finish();

    let mut page_ids = Vec::new();
    for ops in page_streams {
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
        page_ids.push(page_id);
    }

    let count = page_ids.len().max(1) as i64;
    let kids: Vec<Object> = page_ids.into_iter().map(Object::Reference).collect();

    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => kids,
        "Count" => count,
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
