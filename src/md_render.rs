//! Inline Markdown → styled spans for DOCX and PDF export.

use crate::pdf_metrics::{char_width_pt, text_width_pt, PdfFont};
use docx_rs::{Hyperlink, HyperlinkType, Paragraph, Run, RunFonts};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledSpan {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub hyperlink: Option<String>,
}

/// Parses **inline** Markdown only (suitable for heading/paragraph/cell strings).
pub fn markdown_inline_to_spans(input: &str) -> Vec<StyledSpan> {
    let parser = Parser::new_ext(input, Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES);
    let mut bold_depth: u32 = 0;
    let mut italic_depth: u32 = 0;
    let mut link_url: Option<String> = None;
    let mut out: Vec<StyledSpan> = Vec::new();

    for ev in parser {
        match ev {
            Event::Start(tag) => match tag {
                Tag::Strong => bold_depth += 1,
                Tag::Emphasis => italic_depth += 1,
                Tag::Link { dest_url, .. } => {
                    link_url = Some(dest_url.to_string());
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Strong => bold_depth = bold_depth.saturating_sub(1),
                TagEnd::Emphasis => italic_depth = italic_depth.saturating_sub(1),
                TagEnd::Link => link_url = None,
                _ => {}
            },
            Event::Text(t) => {
                let s = t.into_string();
                if s.is_empty() {
                    continue;
                }
                out.push(StyledSpan {
                    text: s,
                    bold: bold_depth > 0,
                    italic: italic_depth > 0,
                    code: false,
                    hyperlink: link_url.clone(),
                });
            }
            Event::Code(t) => {
                let s = t.into_string();
                if !s.is_empty() {
                    out.push(StyledSpan {
                        text: s,
                        bold: bold_depth > 0,
                        italic: italic_depth > 0,
                        code: true,
                        hyperlink: None,
                    });
                }
            }
            Event::SoftBreak => out.push(StyledSpan {
                text: " ".to_string(),
                bold: false,
                italic: false,
                code: false,
                hyperlink: None,
            }),
            Event::HardBreak => out.push(StyledSpan {
                text: "\n".to_string(),
                bold: false,
                italic: false,
                code: false,
                hyperlink: None,
            }),
            _ => {}
        }
    }
    merge_adjacent_spans(out)
}

pub fn normalize_for_pdf_builtin_fonts(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\u{2014}' | '\u{2015}' | '\u{2012}' => out.push_str("--"),
            '\u{2013}' | '\u{2212}' => out.push('-'),
            '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{02bc}' => out.push('\''),
            '\u{201c}' | '\u{201d}' | '\u{201e}' => out.push('"'),
            '\u{00ab}' | '\u{00bb}' => out.push('"'),
            '\u{00a0}' | '\u{202f}' | '\u{2007}' => out.push(' '),
            '\u{2026}' => out.push_str("..."),
            '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}' => {}
            '\u{2022}' => out.push('-'),
            c if c.is_control() && c != '\t' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

pub fn merge_adjacent_spans(spans: Vec<StyledSpan>) -> Vec<StyledSpan> {
    let mut merged: Vec<StyledSpan> = Vec::new();
    for s in spans {
        if let Some(last) = merged.last_mut() {
            if last.bold == s.bold
                && last.italic == s.italic
                && last.code == s.code
                && last.hyperlink == s.hyperlink
            {
                last.text.push_str(&s.text);
                continue;
            }
        }
        merged.push(s);
    }
    merged
}

pub fn paragraph_from_markdown(text: &str) -> Paragraph {
    let spans = markdown_inline_to_spans(text);
    paragraph_from_spans(&spans)
}

pub fn paragraph_from_spans(spans: &[StyledSpan]) -> Paragraph {
    if spans.is_empty() {
        return Paragraph::new().add_run(Run::new().add_text(""));
    }
    let mut p = Paragraph::new();
    for sp in spans {
        if sp.text == "\n" {
            p = p.add_run(Run::new().add_break(docx_rs::BreakType::TextWrapping));
            continue;
        }
        if let Some(url) = &sp.hyperlink {
            let run = Run::new()
                .add_text(sp.text.clone())
                .color("0563C1")
                .underline("single");
            let link = Hyperlink::new(url, HyperlinkType::External).add_run(run);
            p = p.add_hyperlink(link);
            continue;
        }
        let mut r = Run::new().add_text(sp.text.clone());
        if sp.bold {
            r = r.bold();
        }
        if sp.italic {
            r = r.italic();
        }
        if sp.code {
            r = r.fonts(RunFonts::new().ascii("Consolas"));
        }
        p = p.add_run(r);
    }
    p
}

/// Greedy word-wrap using measured PDF glyph widths (prose; not for monospace code).
pub fn wrap_spans_to_lines_pdf(
    spans: &[StyledSpan],
    font_size_pt: f32,
    max_width_pt: f32,
) -> Vec<Vec<StyledSpan>> {
    if max_width_pt <= 1.0 {
        return vec![spans.to_vec()];
    }

    #[derive(Clone)]
    struct Piece {
        text: String,
        style: StyledSpan,
    }

    let mut pieces: Vec<Piece> = Vec::new();
    for sp in spans {
        if sp.text.contains('\n') {
            let mut first = true;
            for seg in sp.text.split('\n') {
                if !first {
                    pieces.push(Piece {
                        text: "\n".to_string(),
                        style: StyledSpan {
                            text: String::new(),
                            bold: false,
                            italic: false,
                            code: false,
                            hyperlink: None,
                        },
                    });
                }
                first = false;
                for w in seg.split_whitespace() {
                    if w.is_empty() {
                        continue;
                    }
                    pieces.push(Piece {
                        text: w.to_string(),
                        style: sp.clone(),
                    });
                }
            }
            continue;
        }
        for w in sp.text.split_whitespace() {
            if w.is_empty() {
                continue;
            }
            pieces.push(Piece {
                text: w.to_string(),
                style: sp.clone(),
            });
        }
    }

    let space_font = PdfFont::TimesRoman;
    let space_w = text_width_pt(" ", space_font, font_size_pt).max(font_size_pt * 0.25);

    let mut lines: Vec<Vec<StyledSpan>> = vec![];
    let mut current: Vec<StyledSpan> = vec![];

    for pc in pieces {
        if pc.text == "\n" {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            lines.push(vec![]);
            continue;
        }

        let f = PdfFont::from_span(pc.style.bold, pc.style.italic, pc.style.code);
        let w = text_width_pt(&pc.text, f, font_size_pt);
        let piece = StyledSpan {
            text: pc.text.clone(),
            bold: pc.style.bold,
            italic: pc.style.italic,
            code: pc.style.code,
            hyperlink: pc.style.hyperlink.clone(),
        };

        if current.is_empty() {
            if w <= max_width_pt + 1e-3 {
                current.push(piece);
            } else {
                for chunk in hard_break_word(&piece.text, f, font_size_pt, max_width_pt) {
                    let mut sp = piece.clone();
                    sp.text = chunk;
                    lines.push(vec![sp]);
                }
            }
            continue;
        }

        let line_w = line_width_spans(&current, font_size_pt);
        let with_space = line_w + space_w + w;
        if with_space <= max_width_pt + 1e-3 {
            if let Some(last) = current.last_mut() {
                if last.bold == piece.bold
                    && last.italic == piece.italic
                    && last.code == piece.code
                    && last.hyperlink == piece.hyperlink
                {
                    last.text.push(' ');
                    last.text.push_str(&piece.text);
                } else {
                    let mut ns = piece.clone();
                    ns.text = format!(" {}", piece.text);
                    current.push(ns);
                }
            }
        } else {
            lines.push(std::mem::take(&mut current));
            if w <= max_width_pt + 1e-3 {
                current.push(piece);
            } else {
                for chunk in hard_break_word(&piece.text, f, font_size_pt, max_width_pt) {
                    let mut sp = piece.clone();
                    sp.text = chunk;
                    lines.push(vec![sp]);
                }
            }
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(vec![]);
    }
    lines
}

fn line_width_spans(spans: &[StyledSpan], font_size_pt: f32) -> f32 {
    let mut total = 0.0_f32;
    for sp in spans {
        let f = PdfFont::from_span(sp.bold, sp.italic, sp.code);
        total += text_width_pt(&sp.text, f, font_size_pt);
    }
    total
}

fn hard_break_word(s: &str, font: PdfFont, size_pt: f32, max_w: f32) -> Vec<String> {
    if s.is_empty() {
        return vec![];
    }
    let mut out = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        let mut acc = 0.0_f32;
        let mut end_byte = 0usize;
        for (i, ch) in rest.char_indices() {
            let cw = char_width_pt(ch, font, size_pt);
            if acc + cw > max_w + 1e-4 && end_byte > 0 {
                break;
            }
            if acc + cw > max_w + 1e-4 && end_byte == 0 {
                end_byte = i + ch.len_utf8();
                break;
            }
            acc += cw;
            end_byte = i + ch.len_utf8();
        }
        if end_byte == 0 {
            let ch = rest.chars().next().unwrap();
            end_byte = ch.len_utf8();
        }
        out.push(rest[..end_byte].to_string());
        rest = &rest[end_byte..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bold_segment_detected() {
        let spans = markdown_inline_to_spans("Hello **world**");
        let bold: String = spans
            .iter()
            .filter(|s| s.bold)
            .map(|s| s.text.as_str())
            .collect();
        assert!(bold.contains("world"));
        assert!(!bold.contains("**"));
    }

    #[test]
    fn italic_and_code() {
        let spans = markdown_inline_to_spans("A *italic* and `code`");
        assert!(spans.iter().any(|s| s.italic && s.text.contains("italic")));
        assert!(spans.iter().any(|s| s.code && s.text.contains("code")));
    }

    #[test]
    fn pdf_normalizes_em_dash_and_smart_quotes() {
        let s = "a\u{2014}b\u{2013}c\u{2018}d\u{201c}e";
        let n = normalize_for_pdf_builtin_fonts(s);
        assert_eq!(n, "a--b-c'd\"e");
    }

    #[test]
    fn wrap_pdf_produces_multiple_lines() {
        let spans = vec![StyledSpan {
            text: "a".repeat(80),
            bold: false,
            italic: false,
            code: false,
            hyperlink: None,
        }];
        let lines = wrap_spans_to_lines_pdf(&spans, 12.0, 200.0);
        assert!(lines.len() >= 2);
    }

    #[test]
    fn times_m_upper_width_matches_afm() {
        let w = text_width_pt("M", PdfFont::TimesRoman, 12.0);
        // TCPDF-derived WinAnsi width for 'M' in Times-Roman (1/1000 em scale).
        assert!((w - 10.668).abs() < 0.02, "w={}", w);
    }

    #[test]
    fn link_span_has_url() {
        let spans = markdown_inline_to_spans("[hi](https://example.com)");
        assert!(spans.iter().any(|s| s.hyperlink.as_deref() == Some("https://example.com")));
    }
}
