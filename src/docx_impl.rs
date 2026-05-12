//! DOCX generation (docx-rs).

use crate::highlight::{hex_for_kind_code_dark, prepare_code_text, tokenize, TokenKind};
use crate::md_render;
use crate::{Alignment, Block, CalloutKind, DocGenError, Document, ListItem};
use docx_rs::{
    AbstractNumbering, AlignmentType, BorderType, BreakType, Docx, Footer, Header, IndentLevel,
    Level, LevelJc, LevelText, LineSpacing, LineSpacingType, NumPages, NumberFormat, Numbering,
    NumberingId, PageMargin, PageNum, Paragraph, Pic, Run, RunFonts, Shading, ShdType,
    SpecialIndentType, Start, Table, TableBorder, TableBorderPosition, TableBorders, TableCell,
    TableOfContents, TableRow, WidthType,
};
use std::io::Cursor;

fn body_fonts() -> RunFonts {
    RunFonts::new()
        .ascii("Times New Roman")
        .hi_ansi("Times New Roman")
}

fn heading_fonts() -> RunFonts {
    RunFonts::new().ascii("Arial").hi_ansi("Arial")
}

fn heading_size(level: u32) -> usize {
    match level {
        1 => 34,
        2 => 30,
        3 => 27,
        _ => 25,
    }
}

fn body_spacing() -> LineSpacing {
    LineSpacing::new()
        .line_rule(LineSpacingType::Auto)
        .line(276)
        .after(200)
}

fn heading_spacing(level: u32) -> LineSpacing {
    let before = match level {
        1 => 360,
        2 => 300,
        3 => 260,
        _ => 220,
    };
    LineSpacing::new()
        .line_rule(LineSpacingType::Auto)
        .before(before)
        .after(200)
        .line(276)
}

fn cell_paragraph(text: &str) -> Paragraph {
    md_render::paragraph_from_markdown(text)
        .fonts(body_fonts())
        .size(24)
        .line_spacing(
            LineSpacing::new()
                .line_rule(LineSpacingType::Auto)
                .line(276)
                .after(80),
        )
}

fn align_type(a: Option<Alignment>) -> Option<AlignmentType> {
    match a? {
        Alignment::Left => Some(AlignmentType::Left),
        Alignment::Center => Some(AlignmentType::Center),
        Alignment::Right => Some(AlignmentType::Right),
    }
}

fn table_borders() -> TableBorders {
    TableBorders::with_empty()
        .set(
            TableBorder::new(TableBorderPosition::Top)
                .border_type(BorderType::Single)
                .size(4)
                .color("BFBFBF"),
        )
        .set(
            TableBorder::new(TableBorderPosition::Bottom)
                .border_type(BorderType::Single)
                .size(4)
                .color("BFBFBF"),
        )
        .set(
            TableBorder::new(TableBorderPosition::Left)
                .border_type(BorderType::Single)
                .size(4)
                .color("BFBFBF"),
        )
        .set(
            TableBorder::new(TableBorderPosition::Right)
                .border_type(BorderType::Single)
                .size(4)
                .color("BFBFBF"),
        )
        .set(
            TableBorder::new(TableBorderPosition::InsideH)
                .border_type(BorderType::Single)
                .size(4)
                .color("D0D0D0"),
        )
        .set(
            TableBorder::new(TableBorderPosition::InsideV)
                .border_type(BorderType::Single)
                .size(4)
                .color("D0D0D0"),
        )
}

fn header_shading() -> Shading {
    Shading::new()
        .shd_type(ShdType::Clear)
        .fill("D9E1F2")
        .color("auto")
}

fn alt_row_shading() -> Shading {
    Shading::new()
        .shd_type(ShdType::Clear)
        .fill("F2F2F2")
        .color("auto")
}

fn build_abstract_ordered() -> AbstractNumbering {
    let mut a = AbstractNumbering::new(0);
    a.multi_level_type = Some("hybridMultilevel".into());
    for lvl in 0usize..9 {
        let lvl_text = format!("%{}.", lvl + 1);
        a.levels.push(
            Level::new(
                lvl,
                Start::new(1),
                NumberFormat::new("decimal"),
                LevelText::new(&lvl_text),
                LevelJc::new("left"),
            )
            .indent(
                Some(720 + (lvl as i32) * 360),
                Some(SpecialIndentType::Hanging(360)),
                None,
                None,
            ),
        );
    }
    a
}

fn build_abstract_bullet() -> AbstractNumbering {
    let mut a = AbstractNumbering::new(1);
    a.multi_level_type = Some("hybridMultilevel".into());
    for lvl in 0usize..9 {
        a.levels.push(
            Level::new(
                lvl,
                Start::new(1),
                NumberFormat::new("bullet"),
                LevelText::new("•"),
                LevelJc::new("left"),
            )
            .indent(
                Some(720 + (lvl as i32) * 360),
                Some(SpecialIndentType::Hanging(360)),
                None,
                None,
            ),
        );
    }
    a
}

fn add_list_recursive(
    mut docx: Docx,
    items: &[ListItem],
    ordered: bool,
    body: &RunFonts,
    level: usize,
) -> Docx {
    let num_id = if ordered { 1usize } else { 2usize };
    for item in items {
        let mut p = md_render::paragraph_from_markdown(&item.text)
            .fonts(body.clone())
            .size(24)
            .line_spacing(body_spacing())
            .numbering(NumberingId::new(num_id), IndentLevel::new(level));
        if let Some(true) = item.checked {
            p = p.add_run(Run::new().add_text(" [x] ").bold());
        } else if let Some(false) = item.checked {
            p = p.add_run(Run::new().add_text(" [ ] "));
        }
        docx = docx.add_paragraph(p);
        if !item.children.is_empty() {
            docx = add_list_recursive(docx, &item.children, ordered, body, level + 1);
        }
    }
    docx
}

fn code_shade() -> Shading {
    Shading::new()
        .shd_type(ShdType::Clear)
        .fill("1E222A")
        .color("auto")
}

fn code_line_spacing() -> LineSpacing {
    LineSpacing::new()
        .line_rule(LineSpacingType::Exact)
        .line(240)
        .before(0)
        .after(0)
}

/// Borders for code block table (full cell shading provides fill; borders frame it).
fn code_block_table_borders() -> TableBorders {
    TableBorders::with_empty()
        .set(
            TableBorder::new(TableBorderPosition::Top)
                .border_type(BorderType::Single)
                .size(4)
                .color("3D4450"),
        )
        .set(
            TableBorder::new(TableBorderPosition::Bottom)
                .border_type(BorderType::Single)
                .size(4)
                .color("3D4450"),
        )
        .set(
            TableBorder::new(TableBorderPosition::Left)
                .border_type(BorderType::Single)
                .size(18)
                .color("569CD6"),
        )
        .set(
            TableBorder::new(TableBorderPosition::Right)
                .border_type(BorderType::Single)
                .size(4)
                .color("3D4450"),
        )
}

fn code_paragraph_line(prefix: &str, body: &str, language: Option<&str>) -> Paragraph {
    let mono = RunFonts::new().ascii("Consolas").hi_ansi("Consolas");
    let ls = code_line_spacing();
    let mut p = Paragraph::new().line_spacing(ls);
    p.property = p.property.shading(code_shade());
    if !prefix.is_empty() {
        p = p.add_run(
            Run::new()
                .add_text(prefix.to_string())
                .fonts(mono.clone())
                .size(18)
                .color("858585"),
        );
    }
    let mut hl = tokenize(language, body);
    hl.sort_by_key(|s| s.start);
    if hl.is_empty() {
        return p.add_run(
            Run::new()
                .add_text(preserve_leading_nbsp(body))
                .fonts(mono.clone())
                .size(20)
                .color(hex_for_kind_code_dark(TokenKind::Default)),
        );
    }
    let mut last = 0usize;
    for sp in hl {
        let s = sp.start.min(body.len());
        let e = sp.end.min(body.len());
        if last < s {
            let seg = if last == 0 {
                preserve_leading_nbsp(&body[last..s])
            } else {
                body[last..s].to_string()
            };
            p = p.add_run(
                Run::new()
                    .add_text(seg)
                    .fonts(mono.clone())
                    .size(20)
                    .color(hex_for_kind_code_dark(TokenKind::Default)),
            );
        }
        if s < e {
            let kind = sp.kind;
            p = p.add_run(
                Run::new()
                    .add_text(body[s..e].to_string())
                    .fonts(mono.clone())
                    .size(20)
                    .color(hex_for_kind_code_dark(kind)),
            );
        }
        last = e.max(last);
    }
    if last < body.len() {
        p = p.add_run(
            Run::new()
                .add_text(body[last..].to_string())
                .fonts(mono.clone())
                .size(20)
                .color(hex_for_kind_code_dark(TokenKind::Default)),
        );
    }
    p
}

/// Blank code line: keeps row height inside shaded cell (Word collapses empty paragraphs).
fn code_paragraph_blank() -> Paragraph {
    let mono = RunFonts::new().ascii("Consolas").hi_ansi("Consolas");
    let mut p = Paragraph::new()
        .line_spacing(code_line_spacing())
        .add_run(
            Run::new()
                .add_text("\u{00A0}".to_string())
                .fonts(mono)
                .size(20)
                .color("D4D4D4"),
        );
    p.property = p.property.shading(code_shade());
    p
}

fn preserve_leading_nbsp(s: &str) -> String {
    let t = s.trim_end_matches('\r');
    let n = t.chars().take_while(|c| *c == ' ' || *c == '\t').count();
    let rest: String = t.chars().skip(n).collect();
    let lead: String = t
        .chars()
        .take(n)
        .map(|c| if c == '\t' { "    ".to_string() } else { "\u{00A0}".to_string() })
        .collect();
    format!("{lead}{rest}")
}

fn callout_colors(kind: CalloutKind) -> (&'static str, &'static str) {
    match kind {
        CalloutKind::Note => ("E7E6E6", "404040"),
        CalloutKind::Info => ("DEEBF7", "2F5597"),
        CalloutKind::Tip => ("E2EFDA", "375623"),
        CalloutKind::Warning => ("FFF2CC", "7F6000"),
        CalloutKind::Danger => ("FBE5E6", "C00000"),
    }
}

fn callout_label(kind: CalloutKind, title: &Option<String>) -> String {
    if let Some(t) = title {
        return t.clone();
    }
    match kind {
        CalloutKind::Note => "Note".into(),
        CalloutKind::Info => "Info".into(),
        CalloutKind::Tip => "Tip".into(),
        CalloutKind::Warning => "Warning".into(),
        CalloutKind::Danger => "Danger".into(),
    }
}

pub fn build_docx_bytes(doc: &Document) -> Result<Vec<u8>, DocGenError> {
    let body = body_fonts();
    let margin = PageMargin::new()
        .top(1440)
        .bottom(1440)
        .left(1440)
        .right(1440);

    let header = Header::new().add_paragraph(
        Paragraph::new()
            .add_run(Run::new().add_text(doc.metadata.title.clone()).size(20)),
    );
    let footer = Footer::new().add_paragraph(
        Paragraph::new()
            .align(AlignmentType::Center)
            .add_run(Run::new().add_text("Page "))
            .add_page_num(PageNum::new())
            .add_run(Run::new().add_text(" of "))
            .add_num_pages(NumPages::new()),
    );

    let mut docx = Docx::new()
        .default_fonts(body.clone())
        .default_size(24)
        .page_margin(margin)
        .header(header)
        .footer(footer)
        .add_abstract_numbering(build_abstract_ordered())
        .add_abstract_numbering(build_abstract_bullet())
        .add_numbering(Numbering::new(1, 0))
        .add_numbering(Numbering::new(2, 1));

    let mut endnotes: Vec<(String, String)> = Vec::new();

    for block in doc.blocks.iter() {
        match block {
            Block::Heading {
                level,
                text,
                anchor,
            } => {
                let spans = md_render::markdown_inline_to_spans(text);
                let mut p = Paragraph::new().line_spacing(heading_spacing(*level));
                if let Some(a) = anchor {
                    let id = stable_bookmark_id(a);
                    p = p.add_bookmark_start(id, a);
                }
                for sp in spans {
                    let mut r = Run::new()
                        .add_text(sp.text.clone())
                        .fonts(heading_fonts())
                        .size(heading_size(*level));
                    if sp.bold {
                        r = r.bold();
                    }
                    if sp.italic {
                        r = r.italic();
                    }
                    if sp.code {
                        r = r.fonts(RunFonts::new().ascii("Consolas"));
                    }
                    if let Some(url) = &sp.hyperlink {
                        let run = r.color("0563C1").underline("single");
                        let link = docx_rs::Hyperlink::new(url, docx_rs::HyperlinkType::External).add_run(run);
                        p = p.add_hyperlink(link);
                    } else {
                        p = p.add_run(r);
                    }
                }
                if anchor.is_some() {
                    let id = stable_bookmark_id(anchor.as_ref().unwrap());
                    p = p.add_bookmark_end(id);
                }
                p = match level {
                    1 => p.style("Heading1"),
                    2 => p.style("Heading2"),
                    3 => p.style("Heading3"),
                    _ => p.style("Heading4"),
                };
                docx = docx.add_paragraph(p);
            }
            Block::Paragraph { text, align } => {
                let mut p = md_render::paragraph_from_markdown(text)
                    .fonts(body.clone())
                    .size(24)
                    .line_spacing(body_spacing());
                if let Some(at) = align_type(align.as_ref().copied()) {
                    p = p.align(at);
                }
                docx = docx.add_paragraph(p);
            }
            Block::List {
                ordered,
                items,
                start: _,
            } => {
                docx = add_list_recursive(docx, items, *ordered, &body, 0);
            }
            Block::Table {
                header,
                rows,
                alignments,
                caption,
            } => {
                if let Some(c) = caption {
                    docx = docx.add_paragraph(
                        Paragraph::new()
                            .fonts(body.clone())
                            .size(22)
                            .italic()
                            .add_run(Run::new().add_text(c.clone())),
                    );
                }
                let mut t = Table::new(vec![]).set_borders(table_borders());
                let mut header_cells = vec![];
                for (i, h) in header.iter().enumerate() {
                    let mut cp = cell_paragraph(h);
                    if let Some(a) =
                        align_type(alignments.as_ref().and_then(|v| v.get(i).copied()))
                    {
                        cp = cp.align(a);
                    }
                    header_cells.push(
                        TableCell::new()
                            .shading(header_shading())
                            .add_paragraph(cp),
                    );
                }
                t = t.add_row(TableRow::new(header_cells));
                for (ri, r) in rows.iter().enumerate() {
                    let mut cells = vec![];
                    for (ci, c) in r.iter().enumerate() {
                        let mut cell = TableCell::new();
                        if ri % 2 == 1 {
                            cell = cell.shading(alt_row_shading());
                        }
                        let mut cp = cell_paragraph(c);
                        if let Some(a) =
                            align_type(alignments.as_ref().and_then(|v| v.get(ci).copied()))
                        {
                            cp = cp.align(a);
                        }
                        cell = cell.add_paragraph(cp);
                        cells.push(cell);
                    }
                    t = t.add_row(TableRow::new(cells));
                }
                docx = docx.add_table(t);
            }
            Block::Code {
                language,
                text,
                show_line_numbers,
                caption,
                highlight_lines: _,
            } => {
                if let Some(c) = caption {
                    docx = docx.add_paragraph(
                        Paragraph::new()
                            .fonts(body.clone())
                            .size(22)
                            .italic()
                            .add_run(Run::new().add_text(c.clone())),
                    );
                }
                let prepared = prepare_code_text(language.as_deref(), text);
                let mut cell = TableCell::new().shading(code_shade());
                if let Some(lang) = language
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    let mut hdr = Paragraph::new()
                        .line_spacing(
                            LineSpacing::new()
                                .line_rule(LineSpacingType::Exact)
                                .line(200)
                                .before(0)
                                .after(0),
                        )
                        .add_run(
                            Run::new()
                                .add_text(lang.to_ascii_lowercase())
                                .fonts(RunFonts::new().ascii("Consolas").hi_ansi("Consolas"))
                                .size(18)
                                .color("858585"),
                        );
                    hdr.property = hdr.property.shading(code_shade());
                    cell = cell.add_paragraph(hdr);
                }
                if prepared.is_empty() {
                    cell = cell.add_paragraph(code_paragraph_blank());
                } else {
                    for (li, raw_line) in prepared.lines().enumerate() {
                        let line_no = (li + 1) as u32;
                        let prefix = if *show_line_numbers {
                            format!("{:>4} │ ", line_no)
                        } else {
                            String::new()
                        };
                        let p = if raw_line.is_empty() {
                            code_paragraph_blank()
                        } else {
                            code_paragraph_line(&prefix, raw_line, language.as_deref())
                        };
                        cell = cell.add_paragraph(p);
                    }
                }
                let code_table = Table::new(vec![TableRow::new(vec![cell])])
                    .width(5000, WidthType::Pct)
                    .set_borders(code_block_table_borders());
                docx = docx.add_table(code_table);
            }
            Block::Math { latex, .. } => {
                let mono = RunFonts::new().ascii("Consolas").hi_ansi("Consolas");
                docx = docx.add_paragraph(
                    Paragraph::new()
                        .line_spacing(body_spacing())
                        .add_run(
                            Run::new()
                                .add_text(latex.clone())
                                .fonts(mono)
                                .size(20),
                        ),
                );
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
                if let (Some(b64), Some(_m)) = (data_base64, mime) {
                    if let Ok(bytes) = base64::Engine::decode(
                        &base64::engine::general_purpose::STANDARD,
                        b64,
                    ) {
                        if let Ok(img) = image::load_from_memory(&bytes) {
                            let (wpx, hpx) = image::GenericImageView::dimensions(&img);
                            let mut png = std::io::Cursor::new(Vec::new());
                            let _: image::ImageResult<()> =
                                img.write_to(&mut png, image::ImageFormat::Png);
                            let mut pic = Pic::new_with_dimensions(png.into_inner(), wpx, hpx);
                            if let (Some(wp), Some(hp)) = (width_pt, height_pt) {
                                let w_emu = docx_rs::from_px((*wp * 96.0 / 72.0) as u32);
                                let h_emu = docx_rs::from_px((*hp * 96.0 / 72.0) as u32);
                                pic.size = (w_emu, h_emu);
                            }
                            let mut p = Paragraph::new()
                                .fonts(body.clone())
                                .size(24)
                                .line_spacing(body_spacing())
                                .add_run(Run::new().add_image(pic));
                            if let Some(at) = align_type(align.as_ref().copied()) {
                                p = p.align(at);
                            }
                            docx = docx.add_paragraph(p);
                            continue;
                        }
                    }
                }
                let alt_text = alt.clone().unwrap_or_else(|| "Image".to_string());
                docx = docx.add_paragraph(
                    Paragraph::new()
                        .fonts(body.clone())
                        .size(24)
                        .line_spacing(body_spacing())
                        .add_run(Run::new().add_text(format!(
                            "[Image: {} (src: {})]",
                            alt_text, src
                        ))),
                );
            }
            Block::PageBreak => {
                docx = docx.add_paragraph(
                    Paragraph::new().add_run(Run::new().add_break(BreakType::Page)),
                );
            }
            Block::Blockquote { text, attribution } => {
                let p = md_render::paragraph_from_markdown(text)
                    .fonts(body.clone())
                    .size(24)
                    .italic()
                    .indent(Some(720), None, None, None)
                    .line_spacing(body_spacing());
                docx = docx.add_paragraph(p);
                if let Some(a) = attribution {
                    docx = docx.add_paragraph(
                        Paragraph::new()
                            .fonts(body.clone())
                            .size(22)
                            .italic()
                            .indent(Some(720), None, None, None)
                            .add_run(Run::new().add_text(format!("— {a}"))),
                    );
                }
            }
            Block::HorizontalRule => {
                docx = docx.add_paragraph(
                    Paragraph::new()
                        .align(AlignmentType::Center)
                        .add_run(Run::new().add_text("――――――――――――――――")),
                );
            }
            Block::Callout { kind, title, text } => {
                let (fill, border) = callout_colors(*kind);
                let label = callout_label(*kind, title);
                let mut inner = Table::new(vec![]).set_borders(
                    TableBorders::with_empty().set(
                        TableBorder::new(TableBorderPosition::Left)
                            .border_type(BorderType::Single)
                            .size(24)
                            .color(border),
                    ),
                );
                let cell = TableCell::new()
                    .shading(
                        Shading::new()
                            .shd_type(ShdType::Clear)
                            .fill(fill)
                            .color("auto"),
                    )
                    .add_paragraph(
                        Paragraph::new()
                            .add_run(Run::new().add_text(label).bold().size(22)),
                    )
                    .add_paragraph(md_render::paragraph_from_markdown(text).size(22));
                inner = inner.add_row(TableRow::new(vec![cell]));
                docx = docx.add_table(inner);
            }
            Block::Footnote { id, text } => {
                endnotes.push((id.clone(), text.clone()));
            }
            Block::Toc { max_depth } => {
                let end = max_depth.unwrap_or(3).clamp(1, 9) as usize;
                docx = docx.add_table_of_contents(
                    TableOfContents::new()
                        .heading_styles_range(1, end)
                        .alias("Contents"),
                );
            }
        }
    }

    if !endnotes.is_empty() {
        docx = docx.add_paragraph(
            Paragraph::new()
                .fonts(body.clone())
                .size(28)
                .bold()
                .add_run(Run::new().add_text("Endnotes")),
        );
        for (i, (_id, txt)) in endnotes.iter().enumerate() {
            docx = docx.add_paragraph(
                Paragraph::new()
                    .fonts(body.clone())
                    .size(24)
                    .add_run(Run::new().add_text(format!("{}.\t", i + 1)))
                    .add_run(Run::new().add_text(txt.clone())),
            );
        }
    }

    let mut buf = Vec::new();
    docx.build()
        .pack(Cursor::new(&mut buf))
        .map_err(|e| DocGenError::GenerationError(e.to_string()))?;
    Ok(buf)
}

fn stable_bookmark_id(name: &str) -> usize {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut h);
    (h.finish() % 1_000_000) as usize + 1
}
