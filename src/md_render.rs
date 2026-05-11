//! Inline Markdown → styled spans for DOCX and PDF export.

use docx_rs::{Paragraph, Run, RunFonts};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledSpan {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
}

/// Parses **inline** Markdown only (suitable for heading/paragraph/cell strings).
pub fn markdown_inline_to_spans(input: &str) -> Vec<StyledSpan> {
    let parser = Parser::new_ext(input, Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES);
    let mut bold_depth: u32 = 0;
    let mut italic_depth: u32 = 0;
    let mut out: Vec<StyledSpan> = Vec::new();

    for ev in parser {
        match ev {
            Event::Start(tag) => match tag {
                Tag::Strong => bold_depth += 1,
                Tag::Emphasis => italic_depth += 1,
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Strong => bold_depth = bold_depth.saturating_sub(1),
                TagEnd::Emphasis => italic_depth = italic_depth.saturating_sub(1),
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
                    });
                }
            }
            Event::SoftBreak => out.push(StyledSpan {
                text: " ".to_string(),
                bold: false,
                italic: false,
                code: false,
            }),
            Event::HardBreak => out.push(StyledSpan {
                text: "\n".to_string(),
                bold: false,
                italic: false,
                code: false,
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
            if last.bold == s.bold && last.italic == s.italic && last.code == s.code {
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

/// Greedy word-wrap: split on whitespace; each line is a list of spans with preserved style.
pub fn wrap_spans_to_lines(spans: &[StyledSpan], max_chars: usize) -> Vec<Vec<StyledSpan>> {
    if max_chars == 0 {
        return vec![spans.to_vec()];
    }
    let mut lines: Vec<Vec<StyledSpan>> = vec![];
    let mut current: Vec<StyledSpan> = vec![];
    let mut current_len: usize = 0;

    for sp in spans {
        let words: Vec<&str> = sp.text.split_whitespace().collect();
        if words.is_empty() {
            continue;
        }
        for w in words {
            let mut rest = w.to_string();
            while !rest.is_empty() {
                let room = max_chars.saturating_sub(current_len);
                if room == 0 && !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                    current_len = 0;
                    continue;
                }
                let take = if rest.chars().count() <= room.max(1) {
                    rest.chars().count()
                } else if current.is_empty() {
                    max_chars.max(1)
                } else {
                    0
                };
                if take == 0 {
                    lines.push(std::mem::take(&mut current));
                    current_len = 0;
                    continue;
                }
                let chunk: String = rest.chars().take(take).collect();
                rest = rest.chars().skip(take).collect();
                let piece = if current_len == 0 && current.is_empty() {
                    chunk
                } else {
                    format!(" {}", chunk)
                };
                if let Some(last) = current.last_mut() {
                    if last.bold == sp.bold && last.italic == sp.italic && last.code == sp.code {
                        last.text.push_str(&piece);
                        current_len += piece.chars().count();
                        continue;
                    }
                }
                current.push(StyledSpan {
                    text: piece.clone(),
                    bold: sp.bold,
                    italic: sp.italic,
                    code: sp.code,
                });
                current_len += piece.chars().count();
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
    fn wrap_produces_multiple_lines() {
        let spans = vec![StyledSpan {
            text: "a".repeat(40),
            bold: false,
            italic: false,
            code: false,
        }];
        let lines = wrap_spans_to_lines(&spans, 10);
        assert!(lines.len() >= 2);
    }
}
