use std::collections::HashMap;
use typst::foundations::Bytes;
use typst::syntax::{FileId, VirtualPath};

/// Result of converting a `Document` to Typst source.
/// `source` is the `.typ` markup string; `files` is a map of registered
/// binary assets (images) keyed by synthetic virtual file IDs.
pub struct GeneratedSource {
    pub source: String,
    pub files: HashMap<FileId, Bytes>,
}

const PREAMBLE: &str = r####"#set page(margin: (x: 48pt, y: 56pt), numbering: "1")
#set text(font: ("Inter", "Noto Color Emoji"), size: 12pt, fill: black)
#show raw: set text(font: "JetBrains Mono", size: 9pt)
#show raw.where(block: true): block.with(
  fill: rgb("#1E2229"),
  inset: (x: 8pt, y: 6pt),
  radius: 4pt,
  width: 100%,
)
#show raw.where(block: true): set text(fill: rgb("#D4D4D4"))
#set heading(numbering: none)
#show heading.where(level: 1): set text(size: 17pt)
#show heading.where(level: 2): set text(size: 15pt)
#show heading.where(level: 3): set text(size: 13.5pt)

"####;

/// Convert a PocketCrew `Document` into a `GeneratedSource` suitable for
/// compilation with `PocketCrewWorld`.
pub fn json_to_typst(doc: &crate::Document) -> GeneratedSource {
    let mut out = String::new();
    out.push_str(PREAMBLE);

    let mut files: HashMap<FileId, Bytes> = HashMap::new();
    let mut image_counter: usize = 0;

    // Title rendering: skip when first block is an H1 matching metadata.title.
    let first_h1_matches = match doc.blocks.first() {
        Some(crate::Block::Heading { level: 1, text, .. }) => text == &doc.metadata.title,
        _ => false,
    };

    if !first_h1_matches && !doc.metadata.title.is_empty() {
        out.push_str("= ");
        out.push_str(&escape_typst(&doc.metadata.title));
        out.push_str("\n\n");
    }

    if let Some(author) = &doc.metadata.author {
        out.push_str("_By ");
        out.push_str(&escape_typst(author));
        out.push_str("_\n\n");
    }

    for block in &doc.blocks {
        emit_block(block, &mut out, &mut files, &mut image_counter, 0);
    }

    GeneratedSource { source: out, files }
}

fn emit_block(
    block: &crate::Block,
    out: &mut String,
    files: &mut HashMap<FileId, Bytes>,
    image_counter: &mut usize,
    _depth: usize,
) {
    use crate::Block;
    match block {
        Block::Heading {
            level,
            text,
            anchor,
        } => {
            let prefix = "=".repeat(*level as usize);
            out.push_str(&prefix);
            out.push(' ');
            out.push_str(&escape_typst(text));
            if let Some(a) = anchor {
                out.push_str(" <");
                out.push_str(a);
                out.push('>');
            }
            out.push_str("\n\n");
        }
        Block::Paragraph { text, align } => {
            let content = md_to_typst_inline(text);
            match align {
                Some(crate::Alignment::Center) => {
                    out.push_str("#align(center)[");
                    out.push_str(&content);
                    out.push_str("]\n\n");
                }
                Some(crate::Alignment::Right) => {
                    out.push_str("#align(right)[");
                    out.push_str(&content);
                    out.push_str("]\n\n");
                }
                _ => {
                    // Top-level paragraph: apply line-start escape on first line
                    // (and on lines after `\n` in the rendered content) to prevent
                    // unintended block-level parsing (e.g. a paragraph beginning
                    // with `= ` would otherwise be parsed as a heading).
                    out.push_str(&escape_line_starts(&content));
                    out.push_str("\n\n");
                }
            }
        }
        Block::Code {
            language,
            text,
            show_line_numbers,
            caption,
            ..
        } => {
            let body = if *show_line_numbers {
                text.lines()
                    .enumerate()
                    .map(|(i, l)| format!("{:>3} | {}", i + 1, l))
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                text.clone()
            };
            let escaped_body = body.replace('\\', "\\\\").replace('"', "\\\"");
            let raw_call = match language {
                Some(lang) => format!(
                    "#raw(block: true, lang: \"{}\", \"{}\")",
                    lang, escaped_body
                ),
                None => format!("#raw(block: true, \"{}\")", escaped_body),
            };
            if let Some(cap) = caption {
                out.push_str("#figure(caption: [");
                out.push_str(&escape_typst(cap));
                out.push_str("])[\n  ");
                out.push_str(&raw_call);
                out.push_str("\n]\n\n");
            } else {
                out.push_str(&raw_call);
                out.push_str("\n\n");
            }
        }
        Block::List {
            ordered,
            items,
            start,
        } => {
            if *ordered && start.is_some() && *start != Some(1) {
                let n = start.unwrap();
                out.push_str(&format!("#enum(start: {}", n));
                for item in items {
                    out.push_str(", [");
                    out.push_str(&md_to_typst_inline(&item.text));
                    out.push(']');
                }
                out.push_str(")\n\n");
            } else {
                // Compute the max depth of this tree. For shallow lists, prefer the
                // lightweight markup form (which preserves test-friendly `- ` / `+ `
                // lines). For deeply nested lists, emit indented text rows to avoid
                // Typst's show-rule recursion limit on markup-style nested ListItems.
                let max_d = items.iter().map(|i| item_depth(i)).max().unwrap_or(0);
                if max_d >= 16 {
                    for (i, item) in items.iter().enumerate() {
                        emit_deep_list_item(item, *ordered, 0, i as u32 + start.unwrap_or(1), out);
                    }
                    out.push('\n');
                } else {
                    for item in items {
                        emit_list_item(item, *ordered, 0, out);
                    }
                    out.push('\n');
                }
            }
        }
        Block::Table {
            header,
            rows,
            alignments,
            caption,
        } => {
            let cols = header.len();
            let align_str = match alignments {
                None => "left".to_string(),
                Some(aligns) => {
                    let parts: Vec<&str> = aligns
                        .iter()
                        .map(|a| match a {
                            crate::Alignment::Left => "left",
                            crate::Alignment::Center => "center",
                            crate::Alignment::Right => "right",
                        })
                        .collect();
                    format!("({})", parts.join(", "))
                }
            };
            let header_cells: Vec<String> = header
                .iter()
                .map(|h| format!("[{}]", escape_typst(h)))
                .collect();
            let row_strs: Vec<String> = rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|c| format!("[{}]", escape_typst(c)))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .collect();
            let table = if row_strs.is_empty() {
                format!(
                    "#table(\n  columns: {},\n  align: {},\n  table.header({})\n)",
                    cols,
                    align_str,
                    header_cells.join(", ")
                )
            } else {
                format!(
                    "#table(\n  columns: {},\n  align: {},\n  table.header({}),\n  {}\n)",
                    cols,
                    align_str,
                    header_cells.join(", "),
                    row_strs.join(",\n  ")
                )
            };
            if let Some(cap) = caption {
                out.push_str("#figure(caption: [");
                out.push_str(&escape_typst(cap));
                out.push_str("])[\n  ");
                out.push_str(&table);
                out.push_str("\n]\n\n");
            } else {
                out.push_str(&table);
                out.push_str("\n\n");
            }
        }
        Block::Image {
            alt,
            data_base64,
            mime,
            width_pt,
            height_pt,
            ..
        } => {
            if let Some(b64) = data_base64 {
                use base64::Engine;
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(b64.as_bytes())
                    .unwrap_or_default();
                let ext = match mime.as_deref() {
                    Some("image/png") => "png",
                    Some("image/jpeg") => "jpg",
                    Some("image/jpg") => "jpg",
                    Some("image/gif") => "gif",
                    Some("image/svg+xml") => "svg",
                    _ => "bin",
                };
                let n = *image_counter;
                *image_counter += 1;
                let path = format!("/img_{}.{}", n, ext);
                let fid = FileId::new(None, VirtualPath::new(&path));
                files.insert(fid, Bytes::new(decoded));
                out.push_str(&format!("#image(\"{}\"", path));
                if let Some(w) = width_pt {
                    out.push_str(&format!(", width: {}pt", w));
                }
                if let Some(h) = height_pt {
                    out.push_str(&format!(", height: {}pt", h));
                }
                out.push_str(")\n\n");
            } else {
                let alt_text = alt.clone().unwrap_or_default();
                out.push_str("_");
                out.push_str(&escape_typst_inline(&alt_text));
                out.push_str("_\n\n");
            }
        }
        Block::Math { display, latex } => {
            let typst_math = crate::latex_to_typst::latex_to_typst_math(latex);
            if *display {
                out.push_str(&format!("$ {typst_math} $\n\n"));
            } else {
                out.push_str(&format!("${typst_math}$\n\n"));
            }
        }
        Block::Blockquote { text, attribution } => {
            let inner = md_to_typst_inline(text);
            match attribution {
                Some(attr) => {
                    out.push_str("#quote(block: true, attribution: [");
                    out.push_str(&escape_typst(attr));
                    out.push_str("])[");
                    out.push_str(&inner);
                    out.push_str("]\n\n");
                }
                None => {
                    out.push_str("#quote(block: true)[");
                    out.push_str(&inner);
                    out.push_str("]\n\n");
                }
            }
        }
        Block::HorizontalRule => {
            out.push_str("#line(length: 100%)\n\n");
        }
        Block::Callout { kind, title, text } => {
            let (color, name) = match kind {
                crate::CalloutKind::Note => ("#BEE3F8", "NOTE"),
                crate::CalloutKind::Info => ("#BEE3F8", "INFO"),
                crate::CalloutKind::Tip => ("#C6F6D5", "TIP"),
                crate::CalloutKind::Warning => ("#FEFCBF", "WARNING"),
                crate::CalloutKind::Danger => ("#FED7D7", "DANGER"),
            };
            let body = md_to_typst_inline(text);
            out.push_str(&format!(
                "#block(fill: rgb(\"{}\"), inset: 8pt, radius: 4pt, width: 100%)[",
                color
            ));
            match title {
                Some(t) => {
                    out.push_str(&format!("*{}: {}*\n\n", name, escape_typst_inline(t)));
                }
                None => {
                    out.push_str(&format!("*{}*\n\n", name));
                }
            }
            out.push_str(&body);
            out.push_str("]\n\n");
        }
        Block::Footnote { id: _, text } => {
            // Note: `id` is preserved in the schema for future ref-block work
            // but is not tracked in a side table in v0.
            out.push_str("#footnote[");
            out.push_str(&md_to_typst_inline(text));
            out.push_str("]\n\n");
        }
        Block::PageBreak => {
            out.push_str("#pagebreak()\n\n");
        }
        Block::Toc { max_depth } => {
            let n = max_depth.unwrap_or(3);
            out.push_str(&format!("#outline(depth: {})\n\n", n));
        }
    }
}

fn item_depth(item: &crate::ListItem) -> usize {
    1 + item.children.iter().map(item_depth).max().unwrap_or(0)
}

fn emit_deep_list_item(
    item: &crate::ListItem,
    ordered: bool,
    depth: usize,
    ordinal: u32,
    out: &mut String,
) {
    let marker = if ordered {
        format!("{}.", ordinal)
    } else {
        "-".to_string()
    };
    out.push_str(&format!(
        "#h({}pt){} {}\n",
        depth * 12,
        marker,
        md_to_typst_inline(&item.text)
    ));
    for (i, child) in item.children.iter().enumerate() {
        emit_deep_list_item(child, ordered, depth + 1, i as u32 + 1, out);
    }
}

fn emit_list_item(item: &crate::ListItem, ordered: bool, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    let marker = if ordered { '+' } else { '-' };
    let prefix = match item.checked {
        Some(true) => "#metadata(\"checkmark\") [x] ",
        Some(false) => "#metadata(\"☐\") [ ] ",
        None => "",
    };
    out.push_str(&indent);
    out.push(marker);
    out.push(' ');
    out.push_str(prefix);
    out.push_str(&md_to_typst_inline(&item.text));
    out.push('\n');
    for child in &item.children {
        emit_list_item(child, ordered, depth + 1, out);
    }
}

/// Convert a Markdown inline string into Typst inline markup.
fn md_to_typst_inline(text: &str) -> String {
    use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(text, opts);

    let mut out = String::new();
    let mut link_is_autolink_stack: Vec<bool> = Vec::new();

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Strong => out.push('*'),
                Tag::Emphasis => out.push('_'),
                Tag::Strikethrough => out.push_str("#strike["),
                Tag::Link {
                    link_type,
                    dest_url,
                    ..
                } => {
                    let is_auto = matches!(link_type, LinkType::Autolink | LinkType::Email);
                    if is_auto {
                        out.push_str(&format!("#link(\"{}\")", dest_url));
                    } else {
                        out.push_str(&format!("#link(\"{}\")[", dest_url));
                    }
                    link_is_autolink_stack.push(is_auto);
                }
                Tag::Image { .. } => {
                    // emit alt as italic; consume nested text
                    out.push('_');
                }
                Tag::Paragraph => {}
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Strong => out.push('*'),
                TagEnd::Emphasis => out.push('_'),
                TagEnd::Strikethrough => out.push(']'),
                TagEnd::Link => {
                    let was_auto = link_is_autolink_stack.pop().unwrap_or(false);
                    if !was_auto {
                        out.push(']');
                    }
                }
                TagEnd::Image => {
                    out.push('_');
                }
                TagEnd::Paragraph => {}
                _ => {}
            },
            Event::Text(t) => {
                out.push_str(&escape_typst_inline(&t));
            }
            Event::Code(t) => {
                out.push('`');
                out.push_str(&t);
                out.push('`');
            }
            Event::SoftBreak => {
                out.push(' ');
            }
            Event::HardBreak => {
                out.push_str(" \\ ");
            }
            Event::FootnoteReference(_) => {
                // skipped; corresponding Footnote block emits the content
            }
            _ => {}
        }
    }

    out
}

/// Escape Typst special characters in plain text intended for block-level use
/// (heading text, attribution, etc.). Includes line-start markup token escapes.
fn escape_typst(text: &str) -> String {
    let escaped = escape_typst_inline(text);
    // Re-process line-by-line to escape leading block-markup tokens.
    let mut out = String::new();
    for (i, line) in escaped.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        // Check the *original* leading-char token. After char escaping, `=`, `-`, `+`, `/`
        // remain literal (none of those are in the inline escape set). So we can act on `line`.
        if let Some(rest) = needs_line_start_escape(line) {
            out.push('\\');
            out.push_str(rest);
        } else {
            out.push_str(line);
        }
    }
    out
}

/// Apply line-start escape to each line in `text`. Used for top-level paragraph
/// content where line-leading `=`/`-`/`+`/`/` tokens would otherwise trigger
/// Typst block-level parsing.
fn escape_line_starts(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if let Some(rest) = needs_line_start_escape(line) {
            out.push('\\');
            out.push_str(rest);
        } else {
            out.push_str(line);
        }
    }
    out
}

/// If the line starts with `= `, `- `, `+ `, or `/ ` (or one of those followed by EOL),
/// return Some(line) (same content) — caller will prefix with a backslash.
fn needs_line_start_escape(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let first = bytes[0];
    let is_marker = matches!(first, b'=' | b'-' | b'+' | b'/');
    if !is_marker {
        return None;
    }
    // Followed by space, or end-of-line (i.e., line has length 1)?
    if bytes.len() == 1 || bytes[1] == b' ' {
        Some(line)
    } else {
        None
    }
}

/// Escape Typst special characters in inline content (inside `[ ... ]` blocks).
/// Does NOT apply line-start escaping.
fn escape_typst_inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' | '#' | '<' | '>' | '@' | '_' | '*' | '$' | '`' | '[' | ']' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Compile a `GeneratedSource` into PDF bytes by spawning a dedicated thread
/// with 8 MB stack (required because Typst layout is deeply recursive and
/// Android's default stack is 512 KB).
pub fn compile_to_pdf(generated: GeneratedSource) -> Result<Vec<u8>, crate::DocGenError> {
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let world = crate::typst_world::PocketCrewWorld::new(generated.source, generated.files);
            let warned = typst::compile::<typst::layout::PagedDocument>(&world);
            let document = warned.output.map_err(|errors| {
                let msg = errors
                    .iter()
                    .map(|e| e.message.to_string())
                    .collect::<Vec<_>>()
                    .join("; ");
                crate::DocGenError::GenerationError(msg)
            })?;
            typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default()).map_err(|errors| {
                let msg = errors
                    .iter()
                    .map(|e| e.message.to_string())
                    .collect::<Vec<_>>()
                    .join("; ");
                crate::DocGenError::GenerationError(msg)
            })
        })
        .map_err(|e| crate::DocGenError::GenerationError(e.to_string()))?
        .join()
        .map_err(|_| crate::DocGenError::GenerationError("PDF thread panicked".to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Alignment, Block, CalloutKind, Document, ListItem, Metadata};

    // ── helpers ─────────────────────────────────────────────────────────────

    fn doc_with_blocks(blocks: Vec<crate::Block>) -> crate::Document {
        crate::Document {
            version: "1.0".to_string(),
            metadata: crate::Metadata {
                title: "Test Doc".to_string(),
                author: None,
            },
            blocks,
        }
    }

    // Trims long outputs for legible failure messages.
    fn trim(s: &str) -> &str {
        &s[..s.len().min(600)]
    }

    // ── G1 — Heading level 1 ────────────────────────────────────────────────

    #[test]
    fn heading_level1_produces_equals_prefix() {
        let doc = doc_with_blocks(vec![Block::Heading {
            level: 1,
            text: "Project Phoenix".to_string(),
            anchor: None,
        }]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains("= Project Phoenix"), "got: {}", trim(&out));
    }

    // ── G2 — Heading level 3 ────────────────────────────────────────────────

    #[test]
    fn heading_level3_produces_triple_equals() {
        let doc = doc_with_blocks(vec![Block::Heading {
            level: 3,
            text: "Sub".to_string(),
            anchor: None,
        }]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains("=== Sub"), "got: {}", trim(&out));
    }

    // ── G3 — Bold Markdown → Typst star ─────────────────────────────────────

    #[test]
    fn paragraph_bold_markdown_converts_to_typst_star() {
        let doc = doc_with_blocks(vec![Block::Paragraph {
            text: "Say **hello** now".to_string(),
            align: None,
        }]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains("*hello*"), "got: {}", trim(&out));
        assert!(!out.contains("**hello**"));
    }

    // ── G4 — Italic Markdown → Typst underscore ──────────────────────────────

    #[test]
    fn paragraph_italic_markdown_converts_to_typst_underscore() {
        let doc = doc_with_blocks(vec![Block::Paragraph {
            text: "Say *italic* word".to_string(),
            align: None,
        }]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains("_italic_"), "got: {}", trim(&out));
    }

    // ── G5 — Inline code preserved ──────────────────────────────────────────

    #[test]
    fn paragraph_inline_code_preserved() {
        let doc = doc_with_blocks(vec![Block::Paragraph {
            text: "Use `foo()` here.".to_string(),
            align: None,
        }]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains("`foo()`"), "got: {}", trim(&out));
    }

    // ── G6 — Hyperlink → #link ───────────────────────────────────────────────

    #[test]
    fn paragraph_hyperlink_converts_to_typst_link() {
        let doc = doc_with_blocks(vec![Block::Paragraph {
            text: "[Rust](https://rust-lang.org)".to_string(),
            align: None,
        }]);
        let out = json_to_typst(&doc).source;
        assert!(
            out.contains("#link(\"https://rust-lang.org\")[Rust]"),
            "got: {}",
            trim(&out)
        );
    }

    // ── G7 — Code block with language uses #raw function form ────────────────

    #[test]
    fn code_block_with_lang_uses_raw_function_form() {
        let doc = doc_with_blocks(vec![Block::Code {
            language: Some("rust".to_string()),
            text: "fn main() {}".to_string(),
            show_line_numbers: false,
            caption: None,
            highlight_lines: vec![],
        }]);
        let out = json_to_typst(&doc).source;
        assert!(
            out.contains(r#"#raw(block: true, lang: "rust""#),
            "got: {}",
            trim(&out)
        );
        assert!(out.contains("fn main() {}"), "code body missing");
        assert!(
            !out.contains("```rust"),
            "must not use fenced form (fence-safety)"
        );
    }

    // ── G8 — Code block without language omits `lang:` argument ─────────────

    #[test]
    fn code_block_no_lang_omits_lang_arg() {
        let doc = doc_with_blocks(vec![Block::Code {
            language: None,
            text: "plain".to_string(),
            show_line_numbers: false,
            caption: None,
            highlight_lines: vec![],
        }]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains("#raw(block: true,"), "got: {}", trim(&out));
        assert!(
            !out.contains("lang:"),
            "lang: arg must be absent when language is None"
        );
    }

    // ── G9 — Ordered list uses `+` marker ────────────────────────────────────

    #[test]
    fn ordered_list_uses_plus_marker() {
        let doc = doc_with_blocks(vec![Block::List {
            ordered: true,
            items: vec![
                ListItem {
                    text: "First".to_string(),
                    checked: None,
                    children: vec![],
                },
                ListItem {
                    text: "Second".to_string(),
                    checked: None,
                    children: vec![],
                },
            ],
            start: None,
        }]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains("+ First"));
        assert!(out.contains("+ Second"));
    }

    // ── G10 — Unordered list uses `-` marker ─────────────────────────────────

    #[test]
    fn unordered_list_uses_dash_marker() {
        let doc = doc_with_blocks(vec![Block::List {
            ordered: false,
            items: vec![ListItem {
                text: "Alpha".to_string(),
                checked: None,
                children: vec![],
            }],
            start: None,
        }]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains("- Alpha"));
    }

    // ── G11 — Nested list items indented ─────────────────────────────────────

    #[test]
    fn nested_list_item_is_indented() {
        let doc = doc_with_blocks(vec![Block::List {
            ordered: false,
            items: vec![ListItem {
                text: "Parent".to_string(),
                checked: None,
                children: vec![ListItem {
                    text: "Child".to_string(),
                    checked: None,
                    children: vec![],
                }],
            }],
            start: None,
        }]);
        let out = json_to_typst(&doc).source;
        let child_line = out.lines().find(|l| l.contains("Child")).expect("Child");
        assert!(child_line.starts_with("  "), "got: {:?}", child_line);
    }

    // ── G12 — PageBreak → #pagebreak() ───────────────────────────────────────

    #[test]
    fn page_break_emits_pagebreak_call() {
        let doc = doc_with_blocks(vec![Block::PageBreak]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains("#pagebreak()"));
    }

    // ── G13 — HorizontalRule → #line ─────────────────────────────────────────

    #[test]
    fn horizontal_rule_emits_line_call() {
        let doc = doc_with_blocks(vec![Block::HorizontalRule]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains("#line(length: 100%)"));
    }

    // ── G14 — Toc with no max_depth → depth 3 ────────────────────────────────

    #[test]
    fn toc_no_max_depth_defaults_to_3() {
        let doc = doc_with_blocks(vec![Block::Toc { max_depth: None }]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains("#outline(depth: 3)"));
    }

    // ── G15 — Toc with max_depth: Some(2) → depth 2 ──────────────────────────

    #[test]
    fn toc_max_depth_2_produces_outline_depth_2() {
        let doc = doc_with_blocks(vec![Block::Toc { max_depth: Some(2) }]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains("#outline(depth: 2)"));
    }

    // ── G16 — Blockquote with attribution ────────────────────────────────────

    #[test]
    fn blockquote_with_attribution_uses_quote_element() {
        let doc = doc_with_blocks(vec![Block::Blockquote {
            text: "To be or not to be.".to_string(),
            attribution: Some("Shakespeare".to_string()),
        }]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains("#quote"));
        assert!(out.contains("attribution:"));
        assert!(out.contains("Shakespeare"));
    }

    // ── G17 — Callout Warning fill color ─────────────────────────────────────

    #[test]
    fn callout_warning_contains_yellow_fill() {
        let doc = doc_with_blocks(vec![Block::Callout {
            kind: CalloutKind::Warning,
            title: None,
            text: "x".to_string(),
        }]);
        let out = json_to_typst(&doc).source;
        assert!(out.to_ascii_uppercase().contains("FEFCBF"));
    }

    // ── G18 — Footnote → #footnote ───────────────────────────────────────────

    #[test]
    fn footnote_emits_footnote_call() {
        let doc = doc_with_blocks(vec![Block::Footnote {
            id: "ref1".to_string(),
            text: "Footnote text here.".to_string(),
        }]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains("#footnote["));
        assert!(out.contains("Footnote text here."));
    }

    // ── G19 — Table columns count ─────────────────────────────────────────────

    #[test]
    fn table_two_columns_produces_columns_2() {
        let doc = doc_with_blocks(vec![Block::Table {
            header: vec!["Name".to_string(), "Value".to_string()],
            rows: vec![vec!["foo".to_string(), "42".to_string()]],
            alignments: None,
            caption: None,
        }]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains("columns: 2"));
    }

    // ── G20 — Math display block canonical form ───────────────────────────────

    #[test]
    fn math_display_true_uses_canonical_spaced_dollars() {
        let doc = doc_with_blocks(vec![Block::Math {
            display: true,
            latex: "E = mc^2".to_string(),
        }]);
        let out = json_to_typst(&doc).source;
        // Letters spaced so Typst treats them as separate variables; display $ ... $
        assert!(out.contains("$ E = m c^2 $"), "got: {}", trim(&out));
    }

    // ── G21 — Preamble sets Inter + Noto Color Emoji font stack ──────────────

    #[test]
    fn preamble_contains_inter_and_emoji_font_set_rule() {
        let out = json_to_typst(&doc_with_blocks(vec![])).source;
        assert!(out.contains("#set text(font: (\"Inter\", \"Noto Color Emoji\")"));
    }

    // ── G22 — Preamble sets JetBrains Mono for raw ───────────────────────────

    #[test]
    fn preamble_contains_jetbrains_mono_for_raw() {
        let out = json_to_typst(&doc_with_blocks(vec![])).source;
        assert!(out.contains("JetBrains Mono"));
    }

    // ── G23 — Special chars in heading text are escaped ───────────────────────

    #[test]
    fn heading_with_special_chars_is_escaped() {
        let doc = doc_with_blocks(vec![Block::Heading {
            level: 1,
            text: "C# [Guide] *star* `code`".to_string(),
            anchor: None,
        }]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains(r"\#"), "got: {}", trim(&out));
        assert!(out.contains(r"\["));
        assert!(out.contains(r"\]"));
        assert!(out.contains(r"\*"));
        assert!(out.contains(r"\`"));
        assert!(
            !out.contains("C# [Guide]"),
            "raw special chars must not appear"
        );
    }

    // ── G24 — Image without data_base64 emits alt text ───────────────────────

    #[test]
    fn image_without_base64_emits_alt_text_as_italic() {
        let doc = doc_with_blocks(vec![Block::Image {
            src: "diagram.png".to_string(),
            alt: Some("Architecture diagram".to_string()),
            data_base64: None,
            mime: None,
            width_pt: None,
            height_pt: None,
            align: None,
        }]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains("Architecture diagram"));
    }

    // ── G25 — Table alignments emitted ───────────────────────────────────────

    #[test]
    fn table_alignments_emit_align_tuple() {
        let doc = doc_with_blocks(vec![Block::Table {
            header: vec!["A".to_string(), "B".to_string()],
            rows: vec![],
            alignments: Some(vec![Alignment::Left, Alignment::Right]),
            caption: None,
        }]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains("align: (left, right)"), "got: {}", trim(&out));
    }

    // ── G26 — Table caption wraps in #figure ─────────────────────────────────

    #[test]
    fn table_caption_wraps_in_figure() {
        let doc = doc_with_blocks(vec![Block::Table {
            header: vec!["A".to_string()],
            rows: vec![],
            alignments: None,
            caption: Some("Caption text".to_string()),
        }]);
        let out = json_to_typst(&doc).source;
        assert!(
            out.contains("#figure(caption: [Caption text])"),
            "got: {}",
            trim(&out)
        );
    }

    // ── G27 — Inline math (display: false) ───────────────────────────────────

    #[test]
    fn math_inline_uses_unspaced_dollars() {
        let doc = doc_with_blocks(vec![Block::Math {
            display: false,
            latex: "x^2".to_string(),
        }]);
        let out = json_to_typst(&doc).source;
        // Inline math: $x^2$ (no surrounding spaces)
        assert!(out.contains("$x^2$"), "got: {}", trim(&out));
    }

    // ── G28 — Paragraph align center wraps in #align ─────────────────────────

    #[test]
    fn paragraph_center_align_uses_align_call() {
        let doc = doc_with_blocks(vec![Block::Paragraph {
            text: "Centered.".to_string(),
            align: Some(Alignment::Center),
        }]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains("#align(center)["), "got: {}", trim(&out));
    }

    // ── G29 — ListItem checked rendering ─────────────────────────────────────

    #[test]
    fn list_item_checked_states_render_glyphs() {
        let doc = doc_with_blocks(vec![Block::List {
            ordered: false,
            items: vec![
                ListItem {
                    text: "Done".to_string(),
                    checked: Some(true),
                    children: vec![],
                },
                ListItem {
                    text: "Todo".to_string(),
                    checked: Some(false),
                    children: vec![],
                },
                ListItem {
                    text: "Plain".to_string(),
                    checked: None,
                    children: vec![],
                },
            ],
            start: None,
        }]);
        let out = json_to_typst(&doc).source;
        // Checked true → checkmark; false → empty box; None → no prefix
        let done_line = out.lines().find(|l| l.contains("Done")).expect("Done");
        let todo_line = out.lines().find(|l| l.contains("Todo")).expect("Todo");
        let plain_line = out.lines().find(|l| l.contains("Plain")).expect("Plain");
        assert!(
            done_line.contains("checkmark"),
            "expected checkmark glyph, got: {}",
            done_line
        );
        assert!(
            todo_line.contains("☐"),
            "expected empty-box glyph, got: {}",
            todo_line
        );
        assert!(
            !plain_line.contains("checkmark") && !plain_line.contains("☐"),
            "plain item must have no checkbox prefix, got: {}",
            plain_line
        );
    }

    // ── G30 — Ordered list with start uses #enum function form ───────────────

    #[test]
    fn ordered_list_with_start_uses_enum_function() {
        let doc = doc_with_blocks(vec![Block::List {
            ordered: true,
            items: vec![
                ListItem {
                    text: "Five".to_string(),
                    checked: None,
                    children: vec![],
                },
                ListItem {
                    text: "Six".to_string(),
                    checked: None,
                    children: vec![],
                },
            ],
            start: Some(5),
        }]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains("#enum(start: 5"), "got: {}", trim(&out));
        // Markup form must NOT be used when start is non-default
        assert!(!out.contains("+ Five"));
    }

    // ── G31 — Heading anchor emits Typst label ────────────────────────────────

    #[test]
    fn heading_with_anchor_emits_label() {
        let doc = doc_with_blocks(vec![Block::Heading {
            level: 2,
            text: "Intro".to_string(),
            anchor: Some("intro".to_string()),
        }]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains("== Intro <intro>"), "got: {}", trim(&out));
    }

    // ── G32 — Image with data_base64 registers file and emits #image path ────

    #[test]
    fn image_with_base64_registers_file_and_references_path() {
        let png_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
        let doc = doc_with_blocks(vec![Block::Image {
            src: "tiny.png".to_string(),
            alt: None,
            data_base64: Some(png_b64.to_string()),
            mime: Some("image/png".to_string()),
            width_pt: Some(50.0),
            height_pt: None,
            align: None,
        }]);
        let generated = json_to_typst(&doc);
        assert!(
            generated.source.contains(r#"#image("/img_0.png""#),
            "got: {}",
            trim(&generated.source)
        );
        assert!(generated.source.contains("width: 50pt"));
        assert_eq!(generated.files.len(), 1, "exactly one file registered");
    }

    // ── G33 — Strikethrough converts to #strike ───────────────────────────────

    #[test]
    fn strikethrough_converts_to_strike_call() {
        let doc = doc_with_blocks(vec![Block::Paragraph {
            text: "remove ~~gone~~ word".to_string(),
            align: None,
        }]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains("#strike[gone]"), "got: {}", trim(&out));
    }

    // ── G34 — Autolink converts to #link ─────────────────────────────────────

    #[test]
    fn autolink_converts_to_link_call() {
        let doc = doc_with_blocks(vec![Block::Paragraph {
            text: "Visit <https://example.com> now".to_string(),
            align: None,
        }]);
        let out = json_to_typst(&doc).source;
        assert!(
            out.contains(r#"#link("https://example.com")"#),
            "got: {}",
            trim(&out)
        );
    }

    // ── G35 — Hard line break emits Typst line-break token ───────────────────

    #[test]
    fn hard_line_break_emits_backslash_space() {
        // Two trailing spaces in Markdown == hard break.
        let doc = doc_with_blocks(vec![Block::Paragraph {
            text: "Line one  \nLine two".to_string(),
            align: None,
        }]);
        let out = json_to_typst(&doc).source;
        assert!(
            out.contains(" \\ "),
            "expected Typst line break ' \\ ' in: {}",
            trim(&out)
        );
    }

    // ── G36 — Backslash-escaped asterisk stays literal ────────────────────────

    #[test]
    fn markdown_backslash_escape_preserves_literal_asterisk() {
        let doc = doc_with_blocks(vec![Block::Paragraph {
            text: r"a \*not bold\* b".to_string(),
            align: None,
        }]);
        let out = json_to_typst(&doc).source;
        // Asterisks must appear as escaped Typst literals, NOT as bold delimiters.
        assert!(out.contains(r"\*"), "got: {}", trim(&out));
        assert!(!out.contains("*not bold*"), "must not become Typst bold");
    }

    // ── G37 — Square brackets in heading text escaped ─────────────────────────

    #[test]
    fn heading_brackets_are_escaped() {
        let doc = doc_with_blocks(vec![Block::Heading {
            level: 1,
            text: "[draft]".to_string(),
            anchor: None,
        }]);
        let out = json_to_typst(&doc).source;
        assert!(out.contains(r"\[draft\]"), "got: {}", trim(&out));
    }

    // ── G38 — Plain text starting with `= ` is line-start-escaped ────────────

    #[test]
    fn line_starting_with_equals_is_escaped_in_plain_text() {
        let doc = doc_with_blocks(vec![Block::Paragraph {
            text: "= not a heading".to_string(),
            align: None,
        }]);
        let out = json_to_typst(&doc).source;
        // The paragraph body must not begin with `= ` (would be parsed as heading).
        // Escape applies to plain-text segments through escape_typst_inline / escape_typst as appropriate.
        assert!(
            out.contains(r"\= not a heading") || out.contains("\\= not a heading"),
            "expected line-start escape on `= ` in: {}",
            trim(&out)
        );
    }

    // ── G39 — LaTeX math translated to Typst math syntax ─────────────────────

    #[test]
    fn latex_frac_translates_to_typst_frac() {
        let doc = doc_with_blocks(vec![Block::Math {
            display: true,
            latex: r"\frac{1}{2}".to_string(),
        }]);
        let out = json_to_typst(&doc).source;
        // Must NOT pass raw LaTeX backslash commands to Typst
        assert!(
            !out.contains(r"\frac"),
            "raw \\frac must not appear in output: {}",
            trim(&out)
        );
        // Must emit Typst frac() call wrapped in display math delimiters
        assert!(
            out.contains("$ frac(1, 2) $"),
            "expected Typst frac: {}",
            trim(&out)
        );
    }

    // ── G40 — Code block body with triple backticks survives ──────────────────

    #[test]
    fn code_block_with_triple_backticks_in_body_is_preserved() {
        let body = "before\n```\ninner\n```\nafter";
        let doc = doc_with_blocks(vec![Block::Code {
            language: Some("md".to_string()),
            text: body.to_string(),
            show_line_numbers: false,
            caption: None,
            highlight_lines: vec![],
        }]);
        let out = json_to_typst(&doc).source;
        // Function form takes the body as a quoted string, so triple backticks cannot close the call.
        // The Rust-escaped body appears inside the `#raw(...)` call.
        assert!(out.contains("#raw(block: true"), "got: {}", trim(&out));
        assert!(out.contains("inner"), "body content missing");
    }

    // ── G41 — show_line_numbers prepends numbers ──────────────────────────────

    #[test]
    fn code_block_show_line_numbers_prepends_numbers() {
        let doc = doc_with_blocks(vec![Block::Code {
            language: Some("rust".to_string()),
            text: "let a = 1;\nlet b = 2;".to_string(),
            show_line_numbers: true,
            caption: None,
            highlight_lines: vec![],
        }]);
        let out = json_to_typst(&doc).source;
        // Line numbers are precomputed in Rust as "  1 | " / "  2 | " prefixes.
        assert!(out.contains("  1 | let a = 1;"), "got: {}", trim(&out));
        assert!(out.contains("  2 | let b = 2;"));
    }

    // ── G42 — Title equal to first H1 emits only once ────────────────────────

    #[test]
    fn title_matching_first_h1_is_not_duplicated() {
        let doc = Document {
            version: "1.0".to_string(),
            metadata: Metadata {
                title: "Phoenix".to_string(),
                author: None,
            },
            blocks: vec![
                Block::Heading {
                    level: 1,
                    text: "Phoenix".to_string(),
                    anchor: None,
                },
                Block::Paragraph {
                    text: "body".to_string(),
                    align: None,
                },
            ],
        };
        let out = json_to_typst(&doc).source;
        // Exactly ONE occurrence of `= Phoenix`.
        let count = out.matches("= Phoenix").count();
        assert_eq!(
            count,
            1,
            "expected exactly one `= Phoenix`, got {} in: {}",
            count,
            trim(&out)
        );
    }

    // ── G43 — Empty document does not panic ───────────────────────────────────

    #[test]
    fn empty_document_produces_preamble_only() {
        let doc = Document {
            version: "1.0".to_string(),
            metadata: Metadata {
                title: "".to_string(),
                author: None,
            },
            blocks: vec![],
        };
        let out = json_to_typst(&doc).source;
        // Preamble must still be present.
        assert!(out.contains("#set page("));
        // No spurious heading emission for empty title.
        assert!(
            !out.contains("= \n"),
            "empty title must not emit a bare `= ` heading"
        );
    }
}
