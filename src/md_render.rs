//! Inline Markdown → styled spans for DOCX export.

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
    fn link_span_has_url() {
        let spans = markdown_inline_to_spans("[hi](https://example.com)");
        assert!(spans.iter().any(|s| s.hyperlink.as_deref() == Some("https://example.com")));
    }
}
