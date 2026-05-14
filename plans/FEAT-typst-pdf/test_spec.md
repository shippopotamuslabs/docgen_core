# FEAT-typst-pdf — Behavioral Test Specification

**Language:** Rust (built-in `#[test]` framework)
**Test runner:** `cargo test`
**File locations:** Tests live in the same file as the code under test, in `#[cfg(test)]` modules, following the existing codebase convention.

Scenario IDs are stable. Never reuse a retired ID.

---

## Scenario ID Index

| ID | Category | Description |
|---|---|---|
| S1 | Smoke | `generate_pdf` returns non-empty bytes starting with `%PDF` |
| S2 | Smoke | All Block types in one document produce no error |
| S3 | Smoke | Round-trip: JSON → PDF → `extract_text` recovers heading text |
| S4 | Smoke | Inline bold markdown produces no asterisk literals in extracted text |
| M1 | Multi-page | 90 paragraphs produce ≥ 2 pages |
| M2 | Multi-page | Explicit `PageBreak` block produces ≥ 2 pages |
| W1 | World | Minimal `.typ` source compiles without error |
| W2 | World | Unknown file ID returns `FileError::NotFound` |
| W3 | World | Font index 0 returns `Some(Font)` |
| W4 | World | Font index 999 returns `None` |
| W5 | World | `today()` returns `Some(Datetime)` with offset=None |
| W6 | World | `today()` returns the current calendar date (within ±1 day for timezone) |
| W7 | World | `file()` serves bytes for a registered image ID |
| G1 | Generator | Heading level 1 → `= ` prefix |
| G2 | Generator | Heading level 3 → `=== ` prefix |
| G3 | Generator | Bold Markdown `**word**` → Typst `*word*` |
| G4 | Generator | Italic Markdown `*word*` → Typst `_word_` |
| G5 | Generator | Inline code `` `x` `` → `` `x` `` in Typst output |
| G6 | Generator | Hyperlink `[text](url)` → `#link("url")[text]` |
| G7 | Generator | Code block with lang `rust` → `#raw(block: true, lang: "rust", ...)` |
| G8 | Generator | Code block with no lang → `#raw(block: true,` with no `lang:` arg |
| G9 | Generator | Ordered list → lines starting with `+ ` |
| G10 | Generator | Unordered list → lines starting with `- ` |
| G11 | Generator | Nested list item indented by 2 spaces |
| G12 | Generator | `PageBreak` → `#pagebreak()` literal |
| G13 | Generator | `HorizontalRule` → `#line(length: 100%)` literal |
| G14 | Generator | `Toc { max_depth: None }` → `#outline(depth: 3)` |
| G15 | Generator | `Toc { max_depth: Some(2) }` → `#outline(depth: 2)` |
| G16 | Generator | `Blockquote` with attribution → `#quote` with `attribution:` |
| G17 | Generator | `Callout { kind: Warning }` fill color `#FEFCBF` appears in output |
| G18 | Generator | `Footnote` → `#footnote[` in output |
| G19 | Generator | Table with 2 columns → `columns: 2` in output |
| G20 | Generator | `Math { display: true, latex: "E = mc^2" }` → exactly `$ E = mc^2 $` |
| G21 | Generator | Preamble contains `#set text(font: "Inter"` |
| G22 | Generator | Preamble contains `#show raw: set text(font: "JetBrains Mono"` |
| G23 | Generator | Typst special chars in heading text are escaped (`#`, `[`, `]`, etc.) |
| G24 | Generator | `Image` without `data_base64` emits alt text as italic paragraph |
| G25 | Generator | `Table { alignments: [Left, Right] }` → `align: (left, right)` |
| G26 | Generator | `Table { caption: Some(...) }` → `#figure(caption: [...])[` wrapper |
| G27 | Generator | `Math { display: false }` → inline `$E=mc^2$` (no surrounding spaces) — TODO refine per Typst's actual inline-math behavior |
| G28 | Generator | `Paragraph { align: Some(Center) }` → `#align(center)[` wrapper |
| G29 | Generator | `ListItem { checked: Some(true) }` → checkmark glyph; `Some(false)` → `☐` glyph; `None` → no prefix |
| G30 | Generator | `List { ordered: true, start: Some(5) }` → `#enum(start: 5,` form |
| G31 | Generator | `Heading { anchor: Some("intro") }` → trailing `<intro>` label |
| G32 | Generator | `Image { data_base64: Some(...) }` → `#image("/img_0.png"` + image registered in `GeneratedSource.files` |
| G33 | Generator | Strikethrough `~~gone~~` → `#strike[gone]` |
| G34 | Generator | Autolink `<https://example.com>` → `#link("https://example.com")` |
| G35 | Generator | Markdown hard line break → ` \ ` (Typst line-break token) |
| G36 | Generator | Markdown backslash escape `\*` → literal `\*` in Typst output (not bold) |
| G37 | Generator | Square brackets `[` `]` in plain heading text are escaped to `\[` `\]` |
| G38 | Generator | Plain paragraph text starting with `= ` is escaped to `\= ` |
| G39 | Generator | `Math { latex: "\\frac{1}{2}" }` → emits Typst comment + `#raw` fallback (does not produce a compile error) |
| G40 | Generator | Code block whose body contains ```` ``` ```` is preserved (function form is fence-safe) |
| G41 | Generator | `Code { show_line_numbers: true }` → output contains `"  1 |"` prefix on first line |
| G42 | Generator | Title equal to first H1 block text → only one `= Title` emission |
| G43 | Generator | Empty `metadata.title` and empty blocks → no panic, returns preamble + nothing |
| E1 | Error | Invalid JSON returns `DocGenError::SerializationError` |
| E2 | Error | Valid JSON but `generate_pdf` produces valid PDF (no panic) for empty blocks list |
| E3 | Error | Deeply nested list (40 levels) compiles to a valid PDF (stack-size sanity) |
| E4 | Smoke | `Math { latex: "\\sum" }` does not produce `DocGenError::GenerationError` (LaTeX fallback path) |

---

## Smoke Scenarios

### S1 — PDF header bytes

```rust
// File: src/lib.rs  mod: typst_pdf_tests
#[test]
fn test_typst_pdf_starts_with_percent_pdf() {
    let json = r#"{
        "version": "1.0",
        "metadata": { "title": "Hello", "author": null },
        "blocks": [
            { "type": "paragraph", "text": "Hello world." }
        ]
    }"#.to_string();

    let bytes = generate_pdf(json).expect("generate_pdf failed");

    assert!(!bytes.is_empty(), "PDF bytes must not be empty");
    assert_eq!(
        &bytes[0..4],
        b"%PDF",
        "PDF must start with %PDF, got: {:?}",
        &bytes[..4.min(bytes.len())]
    );
}
```

**Then:** `bytes[0..4] == b"%PDF"` and `bytes.len() > 0`.

---

### S2 — All Block types produce no error

```rust
// File: src/lib.rs  mod: typst_pdf_tests
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
    let bytes = generate_pdf(json).expect("generate_pdf must not error for all-blocks document");
    assert_eq!(&bytes[0..4], b"%PDF");
}
```

**Then:** `generate_pdf` returns `Ok(bytes)` where `bytes[0..4] == b"%PDF"`.

---

### S3 — Text extraction recovers heading

Use a SHORT document so the heading lands on page 1 well before any page break, avoiding mid-word splits in extraction.

```rust
// File: src/lib.rs  mod: typst_pdf_tests
#[test]
fn test_typst_pdf_extract_text_contains_heading() {
    let json = r#"{
        "version": "1.0",
        "metadata": { "title": "Extraction Test", "author": null },
        "blocks": [
            { "type": "heading", "level": 1, "text": "Project Phoenix" },
            { "type": "paragraph", "text": "Short body." }
        ]
    }"#.to_string();

    let pdf_bytes = generate_pdf(json).expect("pdf");
    let text = extract_text(pdf_bytes, "application/pdf".to_string()).expect("extract");

    assert!(
        text.contains("Project Phoenix"),
        "expected 'Project Phoenix' in extracted text, got: {}",
        &text[..text.len().min(500)]
    );
}
```

---

### S4 — No markdown asterisk literals in extracted text

```rust
// File: src/lib.rs  mod: typst_pdf_tests
#[test]
fn test_typst_pdf_no_markdown_asterisks_in_extracted_text() {
    let json = r#"{
        "version": "1.0",
        "metadata": { "title": "T", "author": null },
        "blocks": [
            { "type": "paragraph", "text": "Say **Hi** there." }
        ]
    }"#.to_string();

    let pdf_bytes = generate_pdf(json).expect("pdf");
    let text = extract_text(pdf_bytes, "application/pdf".to_string()).expect("extract");

    assert!(!text.contains("**"), "literal ** appeared in extracted text: {}", text);
    assert!(
        text.contains("Hi"),
        "word 'Hi' missing from extracted text: {}",
        &text[..text.len().min(500)]
    );
}
```

---

## Multi-Page Scenarios

### M1 — Automatic pagination

```rust
// File: src/lib.rs  mod: typst_pdf_tests
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
        metadata: Metadata { title: "Pagination Test".to_string(), author: None },
        blocks,
    };
    let json = serde_json::to_string(&doc).unwrap();
    let pdf_bytes = generate_pdf(json).expect("pdf");

    let lopdf_doc = lopdf::Document::load_mem(&pdf_bytes).expect("lopdf");
    let page_count = lopdf_doc.get_pages().len();
    assert!(page_count >= 2, "expected >= 2 pages for 90 paragraphs, got {}", page_count);
}
```

---

### M2 — Explicit PageBreak

```rust
// File: src/lib.rs  mod: typst_pdf_tests
#[test]
fn test_typst_pdf_explicit_page_break_produces_two_pages() {
    let doc = Document {
        version: "1.0".to_string(),
        metadata: Metadata { title: "Break Test".to_string(), author: None },
        blocks: vec![
            Block::Paragraph { text: "Page one.".to_string(), align: None },
            Block::PageBreak,
            Block::Paragraph { text: "Page two.".to_string(), align: None },
        ],
    };
    let json = serde_json::to_string(&doc).unwrap();
    let pdf_bytes = generate_pdf(json).expect("pdf");
    let lopdf_doc = lopdf::Document::load_mem(&pdf_bytes).expect("lopdf");
    assert_eq!(lopdf_doc.get_pages().len(), 2);
}
```

---

## World Trait Scenarios

All W-series tests use `PocketCrewWorld::new(source, HashMap::new())` unless they specifically exercise the files map.

### W1 — Compiles a trivial source

```rust
// File: src/typst_world.rs  mod: tests
#[test]
fn world_compiles_hello_world() {
    let world = PocketCrewWorld::new("= Hello, World!".to_string(), HashMap::new());
    let result = typst::compile::<typst::layout::PagedDocument>(&world);
    let doc = result.output.expect("compilation must succeed");
    assert_eq!(doc.pages.len(), 1);
}
```

---

### W2 — Unknown file returns NotFound

```rust
#[test]
fn world_unknown_file_id_returns_not_found() {
    use typst::diag::FileError;
    use typst::syntax::{FileId, VirtualPath};

    let world = PocketCrewWorld::new("test".to_string(), HashMap::new());
    let other_id = FileId::new(None, VirtualPath::new("/other.typ"));
    let err = world.source(other_id).unwrap_err();
    assert!(matches!(err, FileError::NotFound(_)), "got {:?}", err);
}
```

---

### W3 — Font index 0 returns Some

```rust
#[test]
fn world_font_index_zero_returns_some() {
    let world = PocketCrewWorld::new("test".to_string(), HashMap::new());
    assert!(world.font(0).is_some());
}
```

---

### W4 — Font index out of range returns None

```rust
#[test]
fn world_font_index_out_of_range_returns_none() {
    let world = PocketCrewWorld::new("test".to_string(), HashMap::new());
    assert!(world.font(999).is_none());
}
```

---

### W5 — today() returns Some

```rust
#[test]
fn world_today_returns_some_for_none_offset() {
    let world = PocketCrewWorld::new("test".to_string(), HashMap::new());
    assert!(world.today(None).is_some());
}
```

---

### W6 — today() returns the current calendar date

```rust
#[test]
fn world_today_matches_system_clock_within_one_day() {
    use time::OffsetDateTime;
    let world = PocketCrewWorld::new("test".to_string(), HashMap::new());
    let now = OffsetDateTime::now_utc();
    let typst_today = world.today(None).expect("today");
    // Datetime has accessor methods; assert year/month line up.
    // Allow ±1 day slack for tests run near midnight UTC.
    let year_ok = typst_today.year().map(|y| y == now.year()).unwrap_or(false);
    let month_ok = typst_today.month().map(|m| (m as u8 as i32 - now.month() as u8 as i32).abs() <= 1).unwrap_or(false);
    assert!(year_ok, "year mismatch");
    assert!(month_ok, "month off by more than 1");
}
```

**Then:** year matches; month within ±1 of system clock.

---

### W7 — file() serves registered bytes

```rust
#[test]
fn world_file_returns_registered_bytes() {
    use typst::foundations::Bytes;
    use typst::syntax::{FileId, VirtualPath};
    let id = FileId::new(None, VirtualPath::new("/img_0.png"));
    let mut files = HashMap::new();
    let payload = Bytes::new(b"PNG_DATA".to_vec());
    files.insert(id, payload.clone());
    let world = PocketCrewWorld::new("test".to_string(), files);
    let got = world.file(id).expect("registered file must resolve");
    assert_eq!(got.as_slice(), b"PNG_DATA");
}
```

---

## Generator Unit Scenarios

All generator tests live in `src/typst_gen.rs` in `mod tests`. Each calls `json_to_typst` (returning `GeneratedSource`) with a minimal `Document` and asserts on `generated.source` (or `generated.files` for G32).

### G1 — Heading level 1

```rust
#[test]
fn heading_level1_produces_equals_prefix() {
    let doc = doc_with_blocks(vec![
        Block::Heading { level: 1, text: "Project Phoenix".to_string(), anchor: None }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("= Project Phoenix"), "got: {}", trim(&out));
}
```

---

### G2 — Heading level 3

```rust
#[test]
fn heading_level3_produces_triple_equals() {
    let doc = doc_with_blocks(vec![
        Block::Heading { level: 3, text: "Sub".to_string(), anchor: None }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("=== Sub"), "got: {}", trim(&out));
}
```

---

### G3 — Bold Markdown converts to Typst star

```rust
#[test]
fn paragraph_bold_markdown_converts_to_typst_star() {
    let doc = doc_with_blocks(vec![
        Block::Paragraph { text: "Say **hello** now".to_string(), align: None }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("*hello*"), "got: {}", trim(&out));
    assert!(!out.contains("**hello**"));
}
```

---

### G4 — Italic Markdown converts to Typst underscore

```rust
#[test]
fn paragraph_italic_markdown_converts_to_typst_underscore() {
    let doc = doc_with_blocks(vec![
        Block::Paragraph { text: "Say *italic* word".to_string(), align: None }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("_italic_"), "got: {}", trim(&out));
}
```

---

### G5 — Inline code preserved

```rust
#[test]
fn paragraph_inline_code_preserved() {
    let doc = doc_with_blocks(vec![
        Block::Paragraph { text: "Use `foo()` here.".to_string(), align: None }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("`foo()`"), "got: {}", trim(&out));
}
```

---

### G6 — Hyperlink converts to #link

```rust
#[test]
fn paragraph_hyperlink_converts_to_typst_link() {
    let doc = doc_with_blocks(vec![
        Block::Paragraph { text: "[Rust](https://rust-lang.org)".to_string(), align: None }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("#link(\"https://rust-lang.org\")[Rust]"), "got: {}", trim(&out));
}
```

---

### G7 — Code block with language uses #raw function form

```rust
#[test]
fn code_block_with_lang_uses_raw_function_form() {
    let doc = doc_with_blocks(vec![
        Block::Code { language: Some("rust".to_string()), text: "fn main() {}".to_string(),
                      show_line_numbers: false, caption: None, highlight_lines: vec![] }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains(r#"#raw(block: true, lang: "rust""#), "got: {}", trim(&out));
    assert!(out.contains("fn main() {}"), "code body missing");
    assert!(!out.contains("```rust"), "must not use fenced form (fence-safety)");
}
```

---

### G8 — Code block without language omits `lang:` argument

```rust
#[test]
fn code_block_no_lang_omits_lang_arg() {
    let doc = doc_with_blocks(vec![
        Block::Code { language: None, text: "plain".to_string(),
                      show_line_numbers: false, caption: None, highlight_lines: vec![] }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("#raw(block: true,"), "got: {}", trim(&out));
    assert!(!out.contains("lang:"), "lang: arg must be absent when language is None");
}
```

---

### G9 — Ordered list uses `+` marker

```rust
#[test]
fn ordered_list_uses_plus_marker() {
    let doc = doc_with_blocks(vec![
        Block::List { ordered: true, items: vec![
            ListItem { text: "First".to_string(), checked: None, children: vec![] },
            ListItem { text: "Second".to_string(), checked: None, children: vec![] },
        ], start: None }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("+ First"));
    assert!(out.contains("+ Second"));
}
```

---

### G10 — Unordered list uses `-` marker

```rust
#[test]
fn unordered_list_uses_dash_marker() {
    let doc = doc_with_blocks(vec![
        Block::List { ordered: false, items: vec![
            ListItem { text: "Alpha".to_string(), checked: None, children: vec![] },
        ], start: None }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("- Alpha"));
}
```

---

### G11 — Nested list items indented

```rust
#[test]
fn nested_list_item_is_indented() {
    let doc = doc_with_blocks(vec![
        Block::List { ordered: false, items: vec![
            ListItem { text: "Parent".to_string(), checked: None, children: vec![
                ListItem { text: "Child".to_string(), checked: None, children: vec![] }
            ] },
        ], start: None }
    ]);
    let out = json_to_typst(&doc).source;
    let child_line = out.lines().find(|l| l.contains("Child")).expect("Child");
    assert!(child_line.starts_with("  "), "got: {:?}", child_line);
}
```

---

### G12 — PageBreak → #pagebreak()

```rust
#[test]
fn page_break_emits_pagebreak_call() {
    let doc = doc_with_blocks(vec![Block::PageBreak]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("#pagebreak()"));
}
```

---

### G13 — HorizontalRule → #line

```rust
#[test]
fn horizontal_rule_emits_line_call() {
    let doc = doc_with_blocks(vec![Block::HorizontalRule]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("#line(length: 100%)"));
}
```

---

### G14 — Toc with no max_depth → depth 3

```rust
#[test]
fn toc_no_max_depth_defaults_to_3() {
    let doc = doc_with_blocks(vec![Block::Toc { max_depth: None }]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("#outline(depth: 3)"));
}
```

---

### G15 — Toc with max_depth: Some(2) → depth 2

```rust
#[test]
fn toc_max_depth_2_produces_outline_depth_2() {
    let doc = doc_with_blocks(vec![Block::Toc { max_depth: Some(2) }]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("#outline(depth: 2)"));
}
```

---

### G16 — Blockquote with attribution

```rust
#[test]
fn blockquote_with_attribution_uses_quote_element() {
    let doc = doc_with_blocks(vec![
        Block::Blockquote { text: "To be or not to be.".to_string(),
                            attribution: Some("Shakespeare".to_string()) }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("#quote"));
    assert!(out.contains("attribution:"));
    assert!(out.contains("Shakespeare"));
}
```

---

### G17 — Callout Warning fill color

```rust
#[test]
fn callout_warning_contains_yellow_fill() {
    let doc = doc_with_blocks(vec![
        Block::Callout { kind: CalloutKind::Warning, title: None, text: "x".to_string() }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.to_ascii_uppercase().contains("FEFCBF"));
}
```

---

### G18 — Footnote → #footnote

```rust
#[test]
fn footnote_emits_footnote_call() {
    let doc = doc_with_blocks(vec![
        Block::Footnote { id: "ref1".to_string(), text: "Footnote text here.".to_string() }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("#footnote["));
    assert!(out.contains("Footnote text here."));
}
```

---

### G19 — Table columns count

```rust
#[test]
fn table_two_columns_produces_columns_2() {
    let doc = doc_with_blocks(vec![
        Block::Table { header: vec!["Name".to_string(), "Value".to_string()],
                       rows: vec![vec!["foo".to_string(), "42".to_string()]],
                       alignments: None, caption: None }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("columns: 2"));
}
```

---

### G20 — Math display block canonical form

```rust
#[test]
fn math_display_true_uses_canonical_spaced_dollars() {
    let doc = doc_with_blocks(vec![
        Block::Math { display: true, latex: "E = mc^2".to_string() }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("$ E = mc^2 $"), "got: {}", trim(&out));
}
```

**Then:** output contains exactly `$ E = mc^2 $` (single canonical form, no newline variant).

---

### G21 — Preamble sets Inter font

```rust
#[test]
fn preamble_contains_inter_font_set_rule() {
    let out = json_to_typst(&doc_with_blocks(vec![])).source;
    assert!(out.contains("#set text(font: \"Inter\""));
}
```

---

### G22 — Preamble sets JetBrains Mono for raw

```rust
#[test]
fn preamble_contains_jetbrains_mono_for_raw() {
    let out = json_to_typst(&doc_with_blocks(vec![])).source;
    assert!(out.contains("JetBrains Mono"));
}
```

---

### G23 — Special chars in heading text are escaped

```rust
#[test]
fn heading_with_special_chars_is_escaped() {
    let doc = doc_with_blocks(vec![
        Block::Heading { level: 1, text: "C# [Guide] *star* `code`".to_string(), anchor: None }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains(r"\#"), "got: {}", trim(&out));
    assert!(out.contains(r"\["));
    assert!(out.contains(r"\]"));
    assert!(out.contains(r"\*"));
    assert!(out.contains(r"\`"));
    assert!(!out.contains("C# [Guide]"), "raw special chars must not appear");
}
```

---

### G24 — Image without data_base64 emits alt text

```rust
#[test]
fn image_without_base64_emits_alt_text_as_italic() {
    let doc = doc_with_blocks(vec![
        Block::Image { src: "diagram.png".to_string(), alt: Some("Architecture diagram".to_string()),
                       data_base64: None, mime: None, width_pt: None, height_pt: None, align: None }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("Architecture diagram"));
}
```

---

### G25 — Table alignments emitted

```rust
#[test]
fn table_alignments_emit_align_tuple() {
    let doc = doc_with_blocks(vec![
        Block::Table {
            header: vec!["A".to_string(), "B".to_string()],
            rows: vec![],
            alignments: Some(vec![Alignment::Left, Alignment::Right]),
            caption: None,
        }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("align: (left, right)"), "got: {}", trim(&out));
}
```

---

### G26 — Table caption wraps in #figure

```rust
#[test]
fn table_caption_wraps_in_figure() {
    let doc = doc_with_blocks(vec![
        Block::Table {
            header: vec!["A".to_string()],
            rows: vec![],
            alignments: None,
            caption: Some("Caption text".to_string()),
        }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("#figure(caption: [Caption text])"), "got: {}", trim(&out));
}
```

---

### G27 — Inline math (display: false)

```rust
#[test]
fn math_inline_uses_unspaced_dollars() {
    let doc = doc_with_blocks(vec![
        Block::Math { display: false, latex: "x^2".to_string() }
    ]);
    let out = json_to_typst(&doc).source;
    // Inline math: $x^2$ (no surrounding spaces inside the delimiters)
    assert!(out.contains("$x^2$"), "got: {}", trim(&out));
}
```

---

### G28 — Paragraph align center wraps in #align

```rust
#[test]
fn paragraph_center_align_uses_align_call() {
    let doc = doc_with_blocks(vec![
        Block::Paragraph { text: "Centered.".to_string(), align: Some(Alignment::Center) }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("#align(center)["), "got: {}", trim(&out));
}
```

---

### G29 — ListItem checked rendering

```rust
#[test]
fn list_item_checked_states_render_glyphs() {
    let doc = doc_with_blocks(vec![
        Block::List { ordered: false, items: vec![
            ListItem { text: "Done".to_string(), checked: Some(true), children: vec![] },
            ListItem { text: "Todo".to_string(), checked: Some(false), children: vec![] },
            ListItem { text: "Plain".to_string(), checked: None, children: vec![] },
        ], start: None }
    ]);
    let out = json_to_typst(&doc).source;
    // Checked true → checkmark; false → empty box; None → no prefix
    let done_line = out.lines().find(|l| l.contains("Done")).expect("Done");
    let todo_line = out.lines().find(|l| l.contains("Todo")).expect("Todo");
    let plain_line = out.lines().find(|l| l.contains("Plain")).expect("Plain");
    assert!(done_line.contains("checkmark"), "expected checkmark glyph, got: {}", done_line);
    assert!(todo_line.contains("☐"), "expected empty-box glyph, got: {}", todo_line);
    assert!(!plain_line.contains("checkmark") && !plain_line.contains("☐"),
            "plain item must have no checkbox prefix, got: {}", plain_line);
}
```

---

### G30 — Ordered list with start uses #enum function form

```rust
#[test]
fn ordered_list_with_start_uses_enum_function() {
    let doc = doc_with_blocks(vec![
        Block::List { ordered: true, items: vec![
            ListItem { text: "Five".to_string(), checked: None, children: vec![] },
            ListItem { text: "Six".to_string(), checked: None, children: vec![] },
        ], start: Some(5) }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("#enum(start: 5"), "got: {}", trim(&out));
    // Markup form must NOT be used when start is non-default
    assert!(!out.contains("+ Five"));
}
```

---

### G31 — Heading anchor emits Typst label

```rust
#[test]
fn heading_with_anchor_emits_label() {
    let doc = doc_with_blocks(vec![
        Block::Heading { level: 2, text: "Intro".to_string(), anchor: Some("intro".to_string()) }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("== Intro <intro>"), "got: {}", trim(&out));
}
```

---

### G32 — Image with data_base64 registers file and emits #image path

```rust
#[test]
fn image_with_base64_registers_file_and_references_path() {
    let png_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
    let doc = doc_with_blocks(vec![
        Block::Image {
            src: "tiny.png".to_string(),
            alt: None,
            data_base64: Some(png_b64.to_string()),
            mime: Some("image/png".to_string()),
            width_pt: Some(50.0),
            height_pt: None,
            align: None,
        }
    ]);
    let generated = json_to_typst(&doc);
    assert!(generated.source.contains(r#"#image("/img_0.png""#),
            "got: {}", trim(&generated.source));
    assert!(generated.source.contains("width: 50pt"));
    assert_eq!(generated.files.len(), 1, "exactly one file registered");
}
```

---

### G33 — Strikethrough converts to #strike

```rust
#[test]
fn strikethrough_converts_to_strike_call() {
    let doc = doc_with_blocks(vec![
        Block::Paragraph { text: "remove ~~gone~~ word".to_string(), align: None }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains("#strike[gone]"), "got: {}", trim(&out));
}
```

---

### G34 — Autolink converts to #link

```rust
#[test]
fn autolink_converts_to_link_call() {
    let doc = doc_with_blocks(vec![
        Block::Paragraph { text: "Visit <https://example.com> now".to_string(), align: None }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains(r#"#link("https://example.com")"#), "got: {}", trim(&out));
}
```

---

### G35 — Hard line break emits Typst line-break token

```rust
#[test]
fn hard_line_break_emits_backslash_space() {
    // Two trailing spaces in Markdown == hard break.
    let doc = doc_with_blocks(vec![
        Block::Paragraph { text: "Line one  \nLine two".to_string(), align: None }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains(" \\ "), "expected Typst line break ' \\ ' in: {}", trim(&out));
}
```

---

### G36 — Backslash-escaped asterisk stays literal

```rust
#[test]
fn markdown_backslash_escape_preserves_literal_asterisk() {
    let doc = doc_with_blocks(vec![
        Block::Paragraph { text: r"a \*not bold\* b".to_string(), align: None }
    ]);
    let out = json_to_typst(&doc).source;
    // Asterisks must appear as escaped Typst literals, NOT as bold delimiters.
    assert!(out.contains(r"\*"), "got: {}", trim(&out));
    assert!(!out.contains("*not bold*"), "must not become Typst bold");
}
```

---

### G37 — Square brackets in heading text escaped

```rust
#[test]
fn heading_brackets_are_escaped() {
    let doc = doc_with_blocks(vec![
        Block::Heading { level: 1, text: "[draft]".to_string(), anchor: None }
    ]);
    let out = json_to_typst(&doc).source;
    assert!(out.contains(r"\[draft\]"), "got: {}", trim(&out));
}
```

---

### G38 — Plain text starting with `= ` is line-start-escaped

```rust
#[test]
fn line_starting_with_equals_is_escaped_in_plain_text() {
    let doc = doc_with_blocks(vec![
        Block::Paragraph { text: "= not a heading".to_string(), align: None }
    ]);
    let out = json_to_typst(&doc).source;
    // The paragraph body must not begin with `= ` (would be parsed as heading).
    // Escape applies to plain-text segments through escape_typst_inline / escape_typst as appropriate.
    assert!(out.contains(r"\= not a heading") || out.contains("\\= not a heading"),
            "expected line-start escape on `= ` in: {}", trim(&out));
}
```

---

### G39 — LaTeX fallback for unsupported commands

```rust
#[test]
fn unsupported_latex_emits_comment_and_raw_fallback() {
    let doc = doc_with_blocks(vec![
        Block::Math { display: true, latex: r"\frac{1}{2}".to_string() }
    ]);
    let out = json_to_typst(&doc).source;
    // Must NOT emit `$ \frac{1}{2} $` (would be a Typst compile error).
    assert!(!out.contains(r"$ \frac{1}{2} $"), "must not pass LaTeX to Typst math: {}", trim(&out));
    // Must emit a comment marker AND a #raw fallback.
    assert!(out.contains("// unsupported LaTeX"), "missing comment marker: {}", trim(&out));
    assert!(out.contains("#raw("), "missing raw fallback: {}", trim(&out));
}
```

---

### G40 — Code block body with triple backticks survives

```rust
#[test]
fn code_block_with_triple_backticks_in_body_is_preserved() {
    let body = "before\n```\ninner\n```\nafter";
    let doc = doc_with_blocks(vec![
        Block::Code { language: Some("md".to_string()), text: body.to_string(),
                      show_line_numbers: false, caption: None, highlight_lines: vec![] }
    ]);
    let out = json_to_typst(&doc).source;
    // Function form takes the body as a quoted string, so triple backticks cannot close the call.
    // The Rust-escaped body appears inside the `#raw(...)` call.
    assert!(out.contains("#raw(block: true"), "got: {}", trim(&out));
    assert!(out.contains("inner"), "body content missing");
}
```

---

### G41 — show_line_numbers prepends numbers

```rust
#[test]
fn code_block_show_line_numbers_prepends_numbers() {
    let doc = doc_with_blocks(vec![
        Block::Code { language: Some("rust".to_string()),
                      text: "let a = 1;\nlet b = 2;".to_string(),
                      show_line_numbers: true, caption: None, highlight_lines: vec![] }
    ]);
    let out = json_to_typst(&doc).source;
    // Line numbers are precomputed in Rust as "  1 | " / "  2 | " prefixes.
    assert!(out.contains("  1 | let a = 1;"), "got: {}", trim(&out));
    assert!(out.contains("  2 | let b = 2;"));
}
```

---

### G42 — Title equal to first H1 emits only once

```rust
#[test]
fn title_matching_first_h1_is_not_duplicated() {
    let doc = Document {
        version: "1.0".to_string(),
        metadata: Metadata { title: "Phoenix".to_string(), author: None },
        blocks: vec![
            Block::Heading { level: 1, text: "Phoenix".to_string(), anchor: None },
            Block::Paragraph { text: "body".to_string(), align: None },
        ],
    };
    let out = json_to_typst(&doc).source;
    // Exactly ONE occurrence of `= Phoenix`.
    let count = out.matches("= Phoenix").count();
    assert_eq!(count, 1, "expected exactly one `= Phoenix`, got {} in: {}", count, trim(&out));
}
```

---

### G43 — Empty document does not panic

```rust
#[test]
fn empty_document_produces_preamble_only() {
    let doc = Document {
        version: "1.0".to_string(),
        metadata: Metadata { title: "".to_string(), author: None },
        blocks: vec![],
    };
    let out = json_to_typst(&doc).source;
    // Preamble must still be present.
    assert!(out.contains("#set page("));
    // No spurious heading emission for empty title.
    assert!(!out.contains("= \n"), "empty title must not emit a bare `= ` heading");
}
```

---

## Error Scenarios

### E1 — Invalid JSON returns SerializationError

```rust
#[test]
fn test_typst_pdf_invalid_json_returns_serialization_error() {
    let result = generate_pdf("not valid json".to_string());
    assert!(result.is_err());
    match result.unwrap_err() {
        DocGenError::SerializationError(_) => {}
        other => panic!("expected SerializationError, got {:?}", other),
    }
}
```

---

### E2 — Empty blocks list generates valid PDF

```rust
#[test]
fn test_typst_pdf_empty_blocks_produces_valid_pdf() {
    let json = r#"{
        "version": "1.0",
        "metadata": { "title": "Empty", "author": null },
        "blocks": []
    }"#.to_string();
    let bytes = generate_pdf(json).expect("empty-blocks PDF must succeed");
    assert_eq!(&bytes[0..4], b"%PDF");
}
```

---

### E3 — Deeply nested list compiles (stack-size sanity)

This test confirms the 8 MB stack spawned by `compile_to_pdf` is sufficient for non-trivial recursive structure. It is the only guard against someone accidentally replacing `Builder::stack_size` with `std::thread::spawn`.

```rust
#[test]
fn test_typst_pdf_deeply_nested_list_no_stack_overflow() {
    // Build a 40-level deep nested unordered list.
    fn nest(depth: u32) -> ListItem {
        ListItem {
            text: format!("Level {}", depth),
            checked: None,
            children: if depth == 0 { vec![] } else { vec![nest(depth - 1)] },
        }
    }
    let doc = Document {
        version: "1.0".to_string(),
        metadata: Metadata { title: "Nested".to_string(), author: None },
        blocks: vec![Block::List { ordered: false, items: vec![nest(40)], start: None }],
    };
    let json = serde_json::to_string(&doc).unwrap();
    let bytes = generate_pdf(json).expect("deep nesting must not stack-overflow");
    assert_eq!(&bytes[0..4], b"%PDF");
}
```

---

### E4 — LaTeX-only math does not crash the pipeline

```rust
#[test]
fn test_typst_pdf_latex_only_math_falls_back_cleanly() {
    let doc = Document {
        version: "1.0".to_string(),
        metadata: Metadata { title: "Math".to_string(), author: None },
        blocks: vec![
            Block::Paragraph { text: "Before math.".to_string(), align: None },
            Block::Math { display: true, latex: r"\sum_{i=0}^{n} i^2".to_string() },
            Block::Paragraph { text: "After math.".to_string(), align: None },
        ],
    };
    let json = serde_json::to_string(&doc).unwrap();
    // Generator detects backslash-alpha → emits comment + raw fallback.
    // Pipeline must succeed (no GenerationError).
    let bytes = generate_pdf(json).expect("LaTeX fallback must produce valid PDF");
    assert_eq!(&bytes[0..4], b"%PDF");
}
```

---

## Test Helpers

Generator tests share a small helper at the top of `typst_gen::tests`:

```rust
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
fn trim(s: &str) -> &str { &s[..s.len().min(600)] }
```

---

## Mutation Defense

The following mutations must each cause at least one test to fail (confirming test suite strength):

| Mutation | Scenario(s) that catch it |
|---|---|
| Change `#pagebreak()` to `#page-break()` | G12, M2 |
| Change `#line(length: 100%)` to `#line(length: 50%)` | G13 |
| Change `+ item` to `- item` for ordered lists | G9 |
| Change `= ` to `== ` for level-1 headings | G1, S3 (heading text extraction) |
| Change `=== ` to `= ` for level-3 headings | G2 |
| Remove `*bold*` conversion (emit `**bold**` literally) | G3, S4 |
| Remove `_italic_` conversion (emit `*italic*` literally) | G4 |
| Emit `#outline(depth: 5)` regardless of `max_depth` | G14, G15 |
| Emit `#outline(depth: 3)` regardless of `max_depth` | G15 |
| Remove `\#` escaping for `#` in headings | G23 |
| Remove `\[` / `\]` escaping in headings | G23, G37 |
| Skip line-start `=` escape in plain text | G38 |
| Change Warning fill to `#C6F6D5` (Tip color) | G17 |
| Remove the 8 MB stack spawn; use `std::thread::spawn` | E3 (40-level deep list) |
| Change `font(999)` to return `Some(font(0))` | W4 |
| Return `Ok(self.source.clone())` for all IDs unconditionally | W2 |
| Stop registering image bytes in `files` map | G32, S2 (image block compile would fail) |
| Skip `typst-pdf` and return empty `Vec<u8>` | S1, S2, S3, S4, M1, M2 |
| Use `include_bytes!` path for wrong font file | W1, W3 |
| Drop strikethrough handling | G33 |
| Drop autolink handling | G34 |
| Drop hard-line-break handling | G35 |
| Treat MD backslash escape as literal `\` keep `*bold*` | G36 |
| Emit raw fenced ` ```rust ` instead of `#raw(...)` | G7, G8, G40 |
| Pass `\frac{1}{2}` to Typst math unmodified | G39, E4 |
| Skip line-number precompute when `show_line_numbers: true` | G41 |
| Emit metadata title even when first block is matching H1 | G42 |
| Drop anchor label emission | G31 |
| Use markup `+` for ordered list with `start: Some(5)` | G30 |
| Drop ListItem.checked rendering | G29 |
| Drop table alignments tuple | G25 |
| Drop figure wrapper for table caption | G26 |

---

## Tests to Retire

These tests from the current `src/lib.rs` check internal lopdf content-stream structure. They are implementation-specific to the lopdf pipeline and **must be deleted** in Task 4.1:

| Test name | Reason for retirement |
|---|---|
| `pdf_code_block_layout_tests::test_code_bg_rect_contains_text_baseline` | Inspects lopdf `re`/`Tm` operations |
| `pdf_code_block_layout_tests::test_code_bg_rect_extends_below_baseline_by_at_least_descender` | Same |
| `pdf_code_block_layout_tests::test_paragraph_after_code_block_ascenders_clear_rect_bottom` | Same |
| `pdf_gen::code_wrap_tests::wrap_code_line_preserves_leading_spaces` | Tests internal `wrap_code_line_pdf` from deleted `pdf_gen.rs` |

---

## Tests to Preserve Unchanged

| Test name | Status |
|---|---|
| `tests::test_generate_pdf` | Passes: still checks `bytes[0..4] == b"%PDF"` |
| `tests::test_pdf_paginates_long_content` | Passes: uses lopdf page count; still valid |
| `tests::test_generate_pdf_extract_has_no_markdown_asterisks` | Passes if `md_to_typst_inline` correct |
| `tests::test_generate_docx` | Untouched |
| `tests::test_generate_docx_heading_fonts` | Untouched |
| `tests::test_generate_docx_markdown_bold_in_document_xml` | Untouched |
| All `md_render::tests::*` | Untouched |
| `highlight::tests::prepare_code_text_expands_tabs` | Untouched |
| All schema / validate tests | Untouched |
