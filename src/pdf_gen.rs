//! PDF generation using `lopdf` with measured wrapping (`pdf_metrics`) and built-in Type1 fonts.

use crate::highlight::{prepare_code_text, rgb_for_kind_code_dark, tokenize};
use crate::md_render::{
    markdown_inline_to_spans, normalize_for_pdf_builtin_fonts, wrap_spans_to_lines_pdf, StyledSpan,
};
use crate::pdf_metrics::{prefix_width_pt, text_width_pt, PdfFont};
use crate::{Alignment, Block, CalloutKind, DocGenError, Document, ListItem};
use base64::Engine;
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document as LopdfDocument, Object, ObjectId, Stream, StringFormat};
use std::collections::BTreeMap;

const PAGE_W: f32 = 595.0;
const PAGE_H: f32 = 842.0;
const MARGIN_L: f32 = 48.0;
const MARGIN_R: f32 = 48.0;
const MARGIN_TOP: f32 = 72.0;
const MARGIN_BOTTOM: f32 = 56.0;

const BODY_PT: f32 = 12.0;
const AUTHOR_PT: f32 = 11.0;
const TITLE_PT: f32 = 20.0;
const CODE_PT: f32 = 9.0;
const CODE_PAD: f32 = 6.0;
const CODE_LINE_NUM_W: f32 = 28.0;
/// Extra fill above the nominal line box so the first code line's ascenders (6.81pt for 9pt
/// Courier) are fully inside the background. Non-first lines are covered by the previous line's
/// background, so only the first line requires this margin.  Needs to exceed ascender − BG_OVERLAP
/// (6.81 − 0.35 = 6.46pt), so 8.0 gives ~1.5pt of headroom.
const CODE_BG_TOP_EXTRA_PT: f32 = 8.0;

fn usable_width_pt() -> f32 {
    (PAGE_W - MARGIN_L - MARGIN_R).max(72.0)
}

fn text_op(s: &str) -> Operation {
    let normalized = normalize_for_pdf_builtin_fonts(s);
    // `lopdf` already escapes literal strings; our old `\\(` layer doubled escapes and
    // showed as visible backslashes. Hex strings avoid `(` / `)` / `\` edge cases in viewers.
    Operation::new(
        "Tj",
        vec![Object::String(
            normalized.into_bytes(),
            StringFormat::Hexadecimal,
        )],
    )
}

#[derive(Clone)]
struct TextChunk {
    text: String,
    size_pt: f32,
    font: PdfFont,
    rgb: (f32, f32, f32),
    #[allow(dead_code)]
    link_uri: Option<String>,
}

fn line_width_chunks(chunks: &[TextChunk]) -> f32 {
    let mut w = 0.0_f32;
    for c in chunks {
        w += text_width_pt(&c.text, c.font, c.size_pt);
    }
    w
}

fn align_offset(align: Option<Alignment>, line_w: f32, area_w: f32) -> f32 {
    match align.unwrap_or(Alignment::Left) {
        Alignment::Left => 0.0,
        Alignment::Center => ((area_w - line_w) * 0.5).max(0.0),
        Alignment::Right => (area_w - line_w).max(0.0),
    }
}

/// One text line: one `Tm`, then per-chunk `rg` / `Tf` / `Tj` (Type1 advances after `Tj`).
fn emit_line_tm(ops: &mut Vec<Operation>, x: f32, y: f32, chunks: &[TextChunk]) {
    if !chunks.iter().any(|c| !c.text.is_empty()) {
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
    for c in chunks {
        if c.text.is_empty() {
            continue;
        }
        ops.push(Operation::new(
            "rg",
            vec![
                c.rgb.0.into(),
                c.rgb.1.into(),
                c.rgb.2.into(),
            ],
        ));
        ops.push(Operation::new(
            "Tf",
            vec![Object::Name(c.font.pdf_name().as_bytes().to_vec()), c.size_pt.into()],
        ));
        ops.push(text_op(&c.text));
    }
}

fn line_height_for_font(pt: f32) -> f32 {
    (pt * 1.35).max(12.0)
}

fn spans_to_chunks(line: &[StyledSpan], base_size: f32) -> Vec<TextChunk> {
    let mut row: Vec<TextChunk> = vec![];
    for seg in line {
        if seg.text == "\n" {
            continue;
        }
        let t = normalize_for_pdf_builtin_fonts(&seg.text.replace('\n', " "));
        if t.is_empty() {
            continue;
        }
        let font = PdfFont::from_span(seg.bold, seg.italic, seg.code);
        let rgb = if seg.hyperlink.is_some() {
            (0.0, 0.27, 0.76)
        } else {
            (0.0, 0.0, 0.0)
        };
        row.push(TextChunk {
            text: t,
            size_pt: base_size,
            font,
            rgb,
            link_uri: seg.hyperlink.clone(),
        });
    }
    row
}

fn line_chunks_from_spans(
    spans: &[StyledSpan],
    base_size: f32,
    max_width_pt: f32,
    _align: Option<Alignment>,
) -> Vec<Vec<TextChunk>> {
    let mut lines_out: Vec<Vec<TextChunk>> = vec![];
    for line in wrap_spans_to_lines_pdf(spans, base_size, max_width_pt) {
        let row = spans_to_chunks(&line, base_size);
        if !row.is_empty() {
            lines_out.push(row);
        }
    }
    if lines_out.is_empty() {
        lines_out.push(vec![]);
    }
    lines_out
}

/// Monospace / code: preserve leading spaces and internal runs; break only on width (no `split_whitespace`).
fn wrap_code_line_pdf(line: &str, font: PdfFont, size_pt: f32, max_width_pt: f32) -> Vec<String> {
    let normalized = normalize_for_pdf_builtin_fonts(line);
    if normalized.is_empty() {
        return vec![String::new()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut rest = normalized.as_str();
    while !rest.is_empty() {
        let n = prefix_width_pt(rest, font, size_pt, max_width_pt).max(1);
        let (a, b) = rest.split_at(n);
        out.push(a.to_string());
        rest = b;
    }
    out
}

fn wrap_plain_pdf(s: &str, font: PdfFont, size_pt: f32, max_width_pt: f32) -> Vec<String> {
    let normalized = normalize_for_pdf_builtin_fonts(s);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();

    for w in normalized.split_whitespace() {
        let test = if current.is_empty() {
            w.to_string()
        } else {
            format!("{current} {w}")
        };
        if text_width_pt(&test, font, size_pt) <= max_width_pt + 1e-3 {
            current = test;
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        if text_width_pt(w, font, size_pt) <= max_width_pt + 1e-3 {
            current = w.to_string();
        } else {
            let mut rest = w;
            while !rest.is_empty() {
                let n = prefix_width_pt(rest, font, size_pt, max_width_pt).max(1);
                let (a, b) = rest.split_at(n);
                lines.push(a.to_string());
                rest = b;
            }
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        let flat: String = normalized.chars().filter(|c| !c.is_control()).collect();
        if !flat.is_empty() {
            let mut rest = flat.as_str();
            while !rest.is_empty() {
                let n = prefix_width_pt(rest, font, size_pt, max_width_pt).max(1);
                let (a, b) = rest.split_at(n);
                lines.push(a.to_string());
                rest = b;
            }
        }
    }
    lines
}

struct PdfTextCanvas {
    pages: Vec<Vec<Operation>>,
    ops: Vec<Operation>,
    y: f32,
    in_text: bool,
    page_no: u32,
}

impl PdfTextCanvas {
    fn new() -> Self {
        Self {
            pages: Vec::new(),
            ops: Vec::new(),
            y: PAGE_H - MARGIN_TOP,
            in_text: false,
            page_no: 1,
        }
    }

    fn begin_text(&mut self) {
        if !self.in_text {
            self.ops.push(Operation::new("BT", vec![]));
            self.in_text = true;
        }
    }

    fn end_text(&mut self) {
        if self.in_text {
            self.ops.push(Operation::new("ET", vec![]));
            self.in_text = false;
        }
    }

    fn draw_footer(&mut self) {
        self.end_text();
        let label = format!("Page {}", self.page_no);
        let f = PdfFont::TimesRoman;
        let sz = 9.0_f32;
        let w = text_width_pt(&label, f, sz);
        let x = PAGE_W - MARGIN_R - w;
        let y = MARGIN_BOTTOM * 0.5;
        self.ops.push(Operation::new("BT", vec![]));
        self.ops.push(Operation::new(
            "Tm",
            vec![1.into(), 0.into(), 0.into(), 1.into(), x.into(), y.into()],
        ));
        self.ops.push(Operation::new("rg", vec![0.4.into(), 0.4.into(), 0.4.into()]));
        self.ops.push(Operation::new(
            "Tf",
            vec![Object::Name(f.pdf_name().as_bytes().to_vec()), sz.into()],
        ));
        self.ops.push(text_op(&label));
        self.ops.push(Operation::new("ET", vec![]));
        self.in_text = false;
    }

    fn flush_page(&mut self) {
        self.draw_footer();
        self.end_text();
        self.pages.push(std::mem::take(&mut self.ops));
        self.y = PAGE_H - MARGIN_TOP;
        self.page_no += 1;
    }

    fn ensure_vertical_room(&mut self, need: f32) {
        if self.y - need < MARGIN_BOTTOM {
            self.flush_page();
        }
    }

    fn emit_line_chunks(&mut self, chunks: &[TextChunk], font_pt: f32, align: Option<Alignment>, left_x: f32, area_w: f32) {
        if !chunks.iter().any(|c| !c.text.is_empty()) {
            return;
        }
        let lh = line_height_for_font(font_pt);
        self.ensure_vertical_room(lh);
        self.begin_text();
        let lw = line_width_chunks(chunks);
        let x = left_x + align_offset(align, lw, area_w);
        emit_line_tm(&mut self.ops, x, self.y, chunks);
        self.y -= lh;
    }

    fn vspace(&mut self, gap_pt: f32) {
        if gap_pt <= 0.0 {
            return;
        }
        self.ensure_vertical_room(gap_pt);
        self.y -= gap_pt;
    }

    fn page_break(&mut self) {
        if self.ops.is_empty() && !self.in_text {
            return;
        }
        self.flush_page();
    }

    fn fill_rect_gray(&mut self, x: f32, y_bottom: f32, w: f32, h: f32, gray: f32) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        self.end_text();
        self.ops.push(Operation::new("q", vec![]));
        self.ops.push(Operation::new("g", vec![gray.into()]));
        self.ops.push(Operation::new(
            "re",
            vec![x.into(), y_bottom.into(), w.into(), h.into()],
        ));
        self.ops.push(Operation::new("f", vec![]));
        self.ops.push(Operation::new("Q", vec![]));
    }

    fn fill_rect_rgb(&mut self, x: f32, y_bottom: f32, w: f32, h: f32, rgb: (f32, f32, f32)) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        self.end_text();
        self.ops.push(Operation::new("q", vec![]));
        self.ops.push(Operation::new(
            "rg",
            vec![rgb.0.into(), rgb.1.into(), rgb.2.into()],
        ));
        self.ops.push(Operation::new(
            "re",
            vec![x.into(), y_bottom.into(), w.into(), h.into()],
        ));
        self.ops.push(Operation::new("f", vec![]));
        self.ops.push(Operation::new("Q", vec![]));
    }

    fn stroke_rect(&mut self, x: f32, y_bottom: f32, w: f32, h: f32, gray: f32, line_w: f32) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        self.end_text();
        self.ops.push(Operation::new("q", vec![]));
        self.ops.push(Operation::new("G", vec![gray.into()]));
        self.ops.push(Operation::new("w", vec![line_w.into()]));
        self.ops.push(Operation::new(
            "re",
            vec![x.into(), y_bottom.into(), w.into(), h.into()],
        ));
        self.ops.push(Operation::new("S", vec![]));
        self.ops.push(Operation::new("Q", vec![]));
    }

    /// Dark code block band + left accent (call after `ensure_vertical_room`, before text).
    fn paint_code_line_background(&mut self, lh: f32) {
        const BG_OVERLAP: f32 = 0.35;
        let h = lh + BG_OVERLAP + CODE_BG_TOP_EXTRA_PT;
        let mut y_bl = self.y - lh;
        // Keep fill above the footer; do not clamp to the body top margin — a line whose
        // baseline sits at `PAGE_H - MARGIN_TOP` needs the band to extend slightly above that
        // y (larger PDF y) for ascenders; clamping `y_bl + h` to `PAGE_H - MARGIN_TOP` shaved the
        // top of the first code row off the dark background.
        let y_min = MARGIN_BOTTOM;
        if y_bl < y_min {
            let cut = y_min - y_bl;
            y_bl = y_min;
            let h = (h - cut).max(0.5);
            let w = usable_width_pt();
            self.fill_rect_rgb(MARGIN_L, y_bl, w, h, (0.118, 0.133, 0.165));
            self.fill_rect_rgb(MARGIN_L, y_bl, 3.0, h, (0.22, 0.42, 0.62));
            return;
        }
        let w = usable_width_pt();
        self.fill_rect_rgb(MARGIN_L, y_bl, w, h, (0.118, 0.133, 0.165));
        self.fill_rect_rgb(MARGIN_L, y_bl, 3.0, h, (0.22, 0.42, 0.62));
    }

    fn finish(mut self) -> Vec<Vec<Operation>> {
        self.draw_footer();
        self.end_text();
        self.pages.push(self.ops);
        self.pages
    }
}

fn list_marker(ordered: bool, depth: usize, n: u32) -> String {
    if !ordered {
        return match depth % 3 {
            0 => "- ".to_string(),
            1 => "* ".to_string(),
            _ => "o ".to_string(),
        };
    }
    match depth % 3 {
        0 => format!("{n}. "),
        1 => {
            let idx = ((n.saturating_sub(1)) % 26) as u8;
            format!("{}. ", (b'a' + idx) as char)
        }
        _ => format!("{n}. "),
    }
}

fn emit_list_pdf(
    canvas: &mut PdfTextCanvas,
    items: &[ListItem],
    ordered: bool,
    depth: usize,
    counter: &mut u32,
    body: f32,
    left_base: f32,
    area_w: f32,
) {
    const INDENT_PER_LEVEL: f32 = 18.0;
    let left = left_base + depth as f32 * INDENT_PER_LEVEL;
    let prefix_w = INDENT_PER_LEVEL + body * 0.45;
    let text_w = (area_w - (left - left_base) - prefix_w).max(48.0);

    for item in items {
        let prefix = if ordered {
            let p = list_marker(ordered, depth, *counter);
            *counter = counter.saturating_add(1);
            p
        } else {
            list_marker(ordered, depth, 0)
        };
        let spans = markdown_inline_to_spans(&normalize_for_pdf_builtin_fonts(&item.text));
        let mut first = true;
        for line in wrap_spans_to_lines_pdf(&spans, body, text_w) {
            let mut row = vec![];
            if first {
                row.push(TextChunk {
                    text: prefix.clone(),
                    size_pt: body,
                    font: PdfFont::TimesRoman,
                    rgb: (0.0, 0.0, 0.0),
                    link_uri: None,
                });
                first = false;
            } else {
                row.push(TextChunk {
                    text: " ".repeat((prefix.chars().count()).max(1)),
                    size_pt: body,
                    font: PdfFont::TimesRoman,
                    rgb: (0.0, 0.0, 0.0),
                    link_uri: None,
                });
            }
            row.extend(spans_to_chunks(&line, body));
            if !row.is_empty() {
                canvas.emit_line_chunks(&row, body, None, left, area_w);
            }
        }
        if !item.children.is_empty() {
            emit_list_pdf(canvas, &item.children, ordered, depth + 1, counter, body, left_base, area_w);
        }
    }
}

fn callout_colors(kind: CalloutKind) -> ((f32, f32, f32), (f32, f32, f32)) {
    match kind {
        CalloutKind::Note => ((0.93, 0.95, 1.0), (0.2, 0.4, 0.85)),
        CalloutKind::Info => ((0.9, 0.96, 1.0), (0.1, 0.45, 0.9)),
        CalloutKind::Tip => ((0.92, 1.0, 0.93), (0.15, 0.55, 0.25)),
        CalloutKind::Warning => ((1.0, 0.96, 0.88), (0.85, 0.45, 0.1)),
        CalloutKind::Danger => ((1.0, 0.92, 0.92), (0.75, 0.1, 0.1)),
    }
}

fn emit_paragraph_markdown(canvas: &mut PdfTextCanvas, text: &str, size: f32, align: Option<Alignment>, left: f32, width: f32) {
    let spans = markdown_inline_to_spans(&normalize_for_pdf_builtin_fonts(text));
    for row in line_chunks_from_spans(&spans, size, width, align) {
        if row.is_empty() {
            continue;
        }
        canvas.emit_line_chunks(&row, size, align, left, width);
    }
}

fn emit_code_block_pdf(
    canvas: &mut PdfTextCanvas,
    language: Option<&str>,
    text: &str,
    show_line_numbers: bool,
    caption: Option<&str>,
) {
    let prepared = prepare_code_text(language, text);
    let mono = PdfFont::Courier;
    let inner_left = MARGIN_L + CODE_PAD + if show_line_numbers { CODE_LINE_NUM_W } else { 0.0 };
    let inner_w = usable_width_pt() - CODE_PAD * 2.0 - if show_line_numbers { CODE_LINE_NUM_W } else { 0.0 };

    if let Some(c) = caption {
        emit_paragraph_markdown(canvas, c, 10.0, None, MARGIN_L, usable_width_pt());
        canvas.vspace(4.0);
    }

    // Body paragraphs leave an open text object (BT); close it before graphics + code lines.
    canvas.end_text();

    let lines: Vec<&str> = prepared.lines().collect();
    let lh = line_height_for_font(CODE_PT);

    for (idx, raw_line) in lines.iter().enumerate() {
        // Tokenize the same string we measure and wrap (WinAnsi-normalized), so indices stay aligned
        // across soft wraps and page breaks.
        let line_src = normalize_for_pdf_builtin_fonts(raw_line);
        let line_tokens = tokenize(language, &line_src);
        let line_no = idx + 1;
        let wrapped = if line_src.is_empty() {
            vec![String::new()]
        } else {
            wrap_code_line_pdf(&line_src, mono, CODE_PT, inner_w)
        };
        let mut is_first_sub = true;
        let mut col_off = 0usize;
        for w in wrapped {
            canvas.ensure_vertical_room(lh);
            canvas.paint_code_line_background(lh);
            canvas.begin_text();
            let mut row: Vec<TextChunk> = vec![];
            if show_line_numbers && is_first_sub {
                let num = format!("{line_no:>4}  ");
                row.push(TextChunk {
                    text: num,
                    size_pt: CODE_PT,
                    font: mono,
                    rgb: (0.45, 0.52, 0.60),
                    link_uri: None,
                });
            } else if show_line_numbers {
                row.push(TextChunk {
                    text: "      ".to_string(),
                    size_pt: CODE_PT,
                    font: mono,
                    rgb: (0.45, 0.52, 0.60),
                    link_uri: None,
                });
            }
            if w.is_empty() {
                row.push(TextChunk {
                    text: " ".to_string(),
                    size_pt: CODE_PT,
                    font: mono,
                    rgb: (0.83, 0.83, 0.83),
                    link_uri: None,
                });
            } else {
                for (ti, ch) in w.char_indices() {
                    let abs = col_off + ti;
                    let kind = line_tokens
                        .iter()
                        .find(|sp| abs >= sp.start && abs < sp.end)
                        .map(|sp| sp.kind)
                        .unwrap_or(crate::highlight::TokenKind::Default);
                    let [r, g, b] = rgb_for_kind_code_dark(kind);
                    row.push(TextChunk {
                        text: ch.to_string(),
                        size_pt: CODE_PT,
                        font: mono,
                        rgb: (r, g, b),
                        link_uri: None,
                    });
                }
            }
            let lw = line_width_chunks(&row);
            let x = inner_left + align_offset(None, lw, inner_w);
            emit_line_tm(&mut canvas.ops, x, canvas.y, &row);
            canvas.y -= lh;
            canvas.end_text();
            col_off += w.len();
            is_first_sub = false;
        }
    }
    // 10 pt gap: enough to keep Times-Roman 12 pt ascenders (≈8.2 pt) clear of the
    // last line's dark background rect, which extends lh_code below the baseline.
    canvas.vspace(10.0);
}

fn pad_cell_to_points(s: &str, font: PdfFont, sz: f32, target_pt: f32) -> String {
    let mut t = normalize_for_pdf_builtin_fonts(s);
    while text_width_pt(&t, font, sz) < target_pt - 0.25 && t.len() < 12000 {
        t.push(' ');
    }
    t
}

fn emit_table_pdf(
    canvas: &mut PdfTextCanvas,
    header: &[String],
    rows: &[Vec<String>],
    alignments: Option<&Vec<crate::Alignment>>,
    caption: Option<&str>,
) {
    if let Some(c) = caption {
        emit_paragraph_markdown(canvas, c, 10.0, None, MARGIN_L, usable_width_pt());
        canvas.vspace(4.0);
    }
    let n = header.len().max(1);
    let col_w = usable_width_pt() / n as f32;
    let cell_inner = (col_w - 8.0).max(24.0);

    fn wrap_row_cells(cells: &[String], bold: bool, cell_inner: f32) -> (Vec<Vec<String>>, usize) {
        let mut wrapped_cols: Vec<Vec<String>> = Vec::new();
        let mut max_lines = 1usize;
        for c in cells.iter() {
            let size = if bold { 10.0 } else { 9.0 };
            let weight = if bold {
                PdfFont::TimesBold
            } else {
                PdfFont::TimesRoman
            };
            let lines = wrap_plain_pdf(&c.replace('\n', " "), weight, size, cell_inner);
            max_lines = max_lines.max(lines.len());
            wrapped_cols.push(lines);
        }
        (wrapped_cols, max_lines)
    }

    fn align_cell_line(
        s: &str,
        font: PdfFont,
        sz: f32,
        col_target: f32,
        align: Option<Alignment>,
    ) -> String {
        match align {
            Some(Alignment::Right) => {
                let mut t = s.to_string();
                while text_width_pt(&t, font, sz) < col_target - 0.25 && t.len() < 12000 {
                    t.insert(0, ' ');
                }
                t
            }
            Some(Alignment::Center) => {
                let lw = text_width_pt(s, font, sz);
                let sp = text_width_pt(" ", font, sz).max(0.01);
                let pad_total = (((col_target - lw) / sp).floor() as usize).min(2000);
                let left = pad_total / 2;
                format!("{}{}", " ".repeat(left), s)
            }
            _ => pad_cell_to_points(s, font, sz, col_target),
        }
    }

    canvas.vspace(4.0);
    let col_target = col_w - 4.0;
    let (hdr_wrapped, hdr_max) = wrap_row_cells(header, true, cell_inner);
    let sz_hdr = 10.0_f32;
    for li in 0..hdr_max {
        let mut row_chunks: Vec<TextChunk> = Vec::new();
        for i in 0..n {
            let raw = hdr_wrapped
                .get(i)
                .and_then(|v| v.get(li))
                .map(String::as_str)
                .unwrap_or("");
            let al = alignments.and_then(|v| v.get(i)).copied();
            let s = align_cell_line(raw, PdfFont::TimesBold, sz_hdr, col_target, al);
            row_chunks.push(TextChunk {
                text: s,
                size_pt: sz_hdr,
                font: PdfFont::TimesBold,
                rgb: (0.0, 0.0, 0.0),
                link_uri: None,
            });
        }
        canvas.emit_line_chunks(&row_chunks, sz_hdr, None, MARGIN_L, usable_width_pt());
    }
    canvas.vspace(4.0);

    for r in rows {
        let mut cells = r.clone();
        if cells.len() != n {
            cells.resize(n, String::new());
        }
        let (wrapped, mx) = wrap_row_cells(&cells, false, cell_inner);
        let sz = 9.0_f32;
        for li in 0..mx {
            let mut row_chunks: Vec<TextChunk> = Vec::new();
            for i in 0..n {
                let raw = wrapped
                    .get(i)
                    .and_then(|v| v.get(li))
                    .map(String::as_str)
                    .unwrap_or("");
                let al = alignments.and_then(|v| v.get(i)).copied();
                let s = align_cell_line(raw, PdfFont::TimesRoman, sz, col_target, al);
                row_chunks.push(TextChunk {
                    text: s,
                    size_pt: sz,
                    font: PdfFont::TimesRoman,
                    rgb: (0.0, 0.0, 0.0),
                    link_uri: None,
                });
            }
            canvas.emit_line_chunks(&row_chunks, sz, None, MARGIN_L, usable_width_pt());
        }
        canvas.vspace(2.0);
    }
    canvas.vspace(8.0);
}

fn push_image_xobject(
    pdf: &mut LopdfDocument,
    images: &mut BTreeMap<Vec<u8>, ObjectId>,
    data_base64: &str,
    mime: &str,
) -> Option<(Vec<u8>, u32, u32)> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64.as_bytes())
        .ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    let (w, h) = image::GenericImageView::dimensions(&img);
    let name = format!("Im{}", images.len() + 1).into_bytes();
    let mime_l = mime.to_ascii_lowercase();
    if mime_l.contains("jpeg") || mime_l.contains("jpg") {
        let id = pdf.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => w as i64,
                "Height" => h as i64,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
                "Filter" => "DCTDecode",
            },
            bytes,
        ));
        images.insert(name.clone(), id);
        return Some((name, w, h));
    }
    let rgb = img.to_rgb8();
    let raw = rgb.into_raw();
    let id = pdf.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => w as i64,
            "Height" => h as i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
            "Filter" => "FlateDecode",
        },
        raw,
    ));
    images.insert(name.clone(), id);
    Some((name, w, h))
}

fn emit_image_do(
    canvas: &mut PdfTextCanvas,
    name: Vec<u8>,
    w_pt: f32,
    h_pt: f32,
    align: Option<Alignment>,
) {
    let area = usable_width_pt();
    let lw = w_pt.min(area);
    let lh = h_pt * (lw / w_pt);
    let x0 = MARGIN_L + align_offset(align, lw, area);
    let y_img_bottom = canvas.y - lh;
    canvas.ensure_vertical_room(lh + 8.0);
    canvas.end_text();
    canvas.ops.push(Operation::new("q", vec![]));
    canvas.ops.push(Operation::new(
        "cm",
        vec![
            lw.into(),
            0.into(),
            0.into(),
            lh.into(),
            x0.into(),
            y_img_bottom.into(),
        ],
    ));
    canvas.ops.push(Operation::new(
        "Do",
        vec![Object::Name(name.clone())],
    ));
    canvas.ops.push(Operation::new("Q", vec![]));
    canvas.y = y_img_bottom - 8.0;
}

pub fn generate_pdf_from_document(doc: &Document) -> Result<Vec<u8>, DocGenError> {
    let mut pdf = LopdfDocument::with_version("1.5");
    let pages_id = pdf.new_object_id();

    let font_ids: [ObjectId; 8] = std::array::from_fn(|i| {
        let (bf, _) = [
            ("Times-Roman", PdfFont::TimesRoman),
            ("Times-Bold", PdfFont::TimesBold),
            ("Times-Italic", PdfFont::TimesItalic),
            ("Times-BoldItalic", PdfFont::TimesBoldItalic),
            ("Courier", PdfFont::Courier),
            ("Courier-Bold", PdfFont::CourierBold),
            ("Courier-Oblique", PdfFont::CourierOblique),
            ("Courier-BoldOblique", PdfFont::CourierBoldOblique),
        ][i];
        pdf.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => bf,
        })
    });

    let mut xobject_map: BTreeMap<Vec<u8>, ObjectId> = BTreeMap::new();
    let mut canvas = PdfTextCanvas::new();

    let title_spans = markdown_inline_to_spans(&normalize_for_pdf_builtin_fonts(&doc.metadata.title));
    for row in line_chunks_from_spans(&title_spans, TITLE_PT, usable_width_pt(), None) {
        if row.is_empty() {
            continue;
        }
        canvas.emit_line_chunks(&row, TITLE_PT, None, MARGIN_L, usable_width_pt());
    }
    canvas.vspace(8.0);

    if let Some(author) = &doc.metadata.author {
        let line = format!("By {}", normalize_for_pdf_builtin_fonts(author));
        let spans = markdown_inline_to_spans(&line);
        for row in line_chunks_from_spans(&spans, AUTHOR_PT, usable_width_pt(), None) {
            if !row.is_empty() {
                canvas.emit_line_chunks(&row, AUTHOR_PT, None, MARGIN_L, usable_width_pt());
            }
        }
        canvas.vspace(14.0);
    } else {
        canvas.vspace(10.0);
    }

    let mut toc_entries: Vec<(u32, String)> = Vec::new();
    for b in &doc.blocks {
        if let Block::Heading { level, text, .. } = b {
            toc_entries.push((*level, text.clone()));
        }
    }

    let mut footnotes: Vec<(String, String)> = Vec::new();

    for block in &doc.blocks {
        match block {
            Block::Heading { level, text, anchor: _ } => {
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
                emit_paragraph_markdown(&mut canvas, text, size, None, MARGIN_L, usable_width_pt());
                canvas.vspace(match level {
                    1 => 10.0,
                    2 => 8.0,
                    _ => 6.0,
                });
            }
            Block::Paragraph { text, align } => {
                emit_paragraph_markdown(&mut canvas, text, BODY_PT, *align, MARGIN_L, usable_width_pt());
                canvas.vspace(8.0);
            }
            Block::List {
                ordered,
                items,
                start,
            } => {
                let mut c = start.unwrap_or(1);
                emit_list_pdf(
                    &mut canvas,
                    items,
                    *ordered,
                    0,
                    &mut c,
                    BODY_PT,
                    MARGIN_L,
                    usable_width_pt(),
                );
                canvas.vspace(6.0);
            }
            Block::Table {
                header,
                rows,
                alignments,
                caption,
            } => {
                emit_table_pdf(
                    &mut canvas,
                    header,
                    rows,
                    alignments.as_ref(),
                    caption.as_deref(),
                );
            }
            Block::Code {
                language,
                text,
                show_line_numbers,
                caption,
                highlight_lines: _,
            } => {
                canvas.vspace(4.0);
                emit_code_block_pdf(
                    &mut canvas,
                    language.as_deref(),
                    text,
                    *show_line_numbers,
                    caption.as_deref(),
                );
            }
            Block::Math { latex, .. } => {
                for line in wrap_plain_pdf(&latex.replace('\n', " "), PdfFont::TimesRoman, 10.0, usable_width_pt()) {
                    canvas.emit_line_chunks(
                        &[TextChunk {
                            text: line,
                            size_pt: 10.0,
                            font: PdfFont::TimesRoman,
                            rgb: (0.0, 0.0, 0.0),
                            link_uri: None,
                        }],
                        10.0,
                        None,
                        MARGIN_L,
                        usable_width_pt(),
                    );
                }
                canvas.vspace(6.0);
            }
            Block::Image {
                src,
                alt,
                data_base64,
                mime,
                width_pt,
                height_pt,
                align,
            } => {
                if let (Some(b64), Some(m)) = (data_base64, mime) {
                    if let Some((name, iw, ih)) = push_image_xobject(&mut pdf, &mut xobject_map, b64, m) {
                        let mut w_pt = width_pt.unwrap_or(iw as f32 * 72.0 / 96.0);
                        let mut h_pt = height_pt.unwrap_or(ih as f32 * 72.0 / 96.0);
                        if width_pt.is_none() ^ height_pt.is_none() {
                            if width_pt.is_some() {
                                h_pt = ih as f32 * (w_pt / iw as f32);
                            } else {
                                w_pt = iw as f32 * (h_pt / ih as f32);
                            }
                        }
                        emit_image_do(&mut canvas, name, w_pt, h_pt, *align);
                        canvas.vspace(6.0);
                        continue;
                    }
                }
                let alt_text = alt.clone().unwrap_or_else(|| "Image".to_string());
                let s = format!("[{}] {}", alt_text, src);
                emit_paragraph_markdown(&mut canvas, &s, 10.0, None, MARGIN_L, usable_width_pt());
                canvas.vspace(6.0);
            }
            Block::PageBreak => {
                canvas.page_break();
            }
            Block::Blockquote { text, attribution } => {
                let left = MARGIN_L + 12.0;
                let w = usable_width_pt() - 12.0;
                canvas.stroke_rect(left - 6.0, canvas.y - 4.0, 2.0, 40.0, 0.6, 0.6);
                emit_paragraph_markdown(&mut canvas, text, BODY_PT, None, left, w);
                if let Some(a) = attribution {
                    emit_paragraph_markdown(&mut canvas, &format!("— {a}"), BODY_PT - 1.0, None, left, w);
                }
                canvas.vspace(8.0);
            }
            Block::HorizontalRule => {
                canvas.vspace(6.0);
                canvas.end_text();
                canvas.ops.push(Operation::new("q", vec![]));
                canvas.ops.push(Operation::new("G", vec![0.65.into()]));
                canvas.ops.push(Operation::new("w", vec![0.8.into()]));
                let y = canvas.y;
                canvas.ops.push(Operation::new(
                    "m",
                    vec![MARGIN_L.into(), y.into()],
                ));
                canvas.ops.push(Operation::new(
                    "l",
                    vec![(PAGE_W - MARGIN_R).into(), y.into()],
                ));
                canvas.ops.push(Operation::new("S", vec![]));
                canvas.ops.push(Operation::new("Q", vec![]));
                canvas.y -= 12.0;
                canvas.vspace(6.0);
            }
            Block::Callout { kind, title, text } => {
                let (fill, border) = callout_colors(*kind);
                let label = match kind {
                    CalloutKind::Note => "Note",
                    CalloutKind::Info => "Info",
                    CalloutKind::Tip => "Tip",
                    CalloutKind::Warning => "Warning",
                    CalloutKind::Danger => "Danger",
                };
                let head = title
                    .as_ref()
                    .map(|t| format!("{label}: {t}"))
                    .unwrap_or_else(|| label.to_string());
                let h_body = line_height_for_font(BODY_PT) * 3.0 + 16.0;
                let fill_gray = ((fill.0 + fill.1 + fill.2) / 3.0).clamp(0.88_f32, 0.97);
                let border_gray = ((border.0 + border.1 + border.2) / 3.0).clamp(0.25_f32, 0.75);
                canvas.fill_rect_gray(MARGIN_L, canvas.y - h_body, usable_width_pt(), h_body, fill_gray);
                canvas.stroke_rect(MARGIN_L, canvas.y - h_body, 3.0, h_body, border_gray, 1.2);
                emit_paragraph_markdown(&mut canvas, &format!("**{head}**"), BODY_PT, None, MARGIN_L + 8.0, usable_width_pt() - 16.0);
                emit_paragraph_markdown(&mut canvas, text, BODY_PT - 0.5, None, MARGIN_L + 8.0, usable_width_pt() - 16.0);
                canvas.vspace(10.0);
            }
            Block::Footnote { id, text } => {
                footnotes.push((id.clone(), text.clone()));
            }
            Block::Toc { max_depth } => {
                let md = max_depth.unwrap_or(3);
                canvas.vspace(8.0);
                emit_paragraph_markdown(&mut canvas, "## Contents", 16.0, None, MARGIN_L, usable_width_pt());
                for (lv, t) in &toc_entries {
                    if *lv <= md {
                        let indent = (*lv).saturating_sub(1) as f32 * 12.0;
                        emit_paragraph_markdown(
                            &mut canvas,
                            &format!("{}- {}", "  ".repeat(*lv as usize), t),
                            11.0,
                            None,
                            MARGIN_L + indent,
                            usable_width_pt() - indent,
                        );
                    }
                }
                canvas.vspace(10.0);
            }
        }
    }

    if !footnotes.is_empty() {
        canvas.vspace(12.0);
        emit_paragraph_markdown(&mut canvas, "## Endnotes", 15.0, None, MARGIN_L, usable_width_pt());
        let mut n = 1u32;
        for (id, txt) in &footnotes {
            emit_paragraph_markdown(
                &mut canvas,
                &format!("**[{n}]** ({id}) {txt}"),
                BODY_PT,
                None,
                MARGIN_L,
                usable_width_pt(),
            );
            n += 1;
        }
    }

    let page_streams: Vec<Vec<Operation>> = canvas.finish();

    let mut font_dict = dictionary! {};
    for (i, fid) in font_ids.iter().enumerate() {
        let key = format!("F{}", i + 1);
        font_dict.set(key, Object::Reference(*fid));
    }

    let mut xobj_dict = dictionary! {};
    for (name, oid) in &xobject_map {
        xobj_dict.set(name.as_slice(), Object::Reference(*oid));
    }

    let resources_id = if xobject_map.is_empty() {
        pdf.add_object(dictionary! {
            "Font" => font_dict,
        })
    } else {
        pdf.add_object(dictionary! {
            "Font" => font_dict,
            "XObject" => xobj_dict,
        })
    };

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
            "Resources" => resources_id,
        });
        page_ids.push(page_id);
    }

    let count = page_ids.len().max(1) as i64;
    let kids: Vec<Object> = page_ids.into_iter().map(Object::Reference).collect();

    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => kids,
        "Count" => count,
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

#[cfg(test)]
mod code_wrap_tests {
    use super::{wrap_code_line_pdf, PdfFont};

    #[test]
    fn wrap_code_line_preserves_leading_spaces() {
        let lines = wrap_code_line_pdf("    let x = 1;", PdfFont::Courier, 9.0, 400.0);
        assert!(lines[0].starts_with("    "), "got {:?}", lines[0]);
    }
}
