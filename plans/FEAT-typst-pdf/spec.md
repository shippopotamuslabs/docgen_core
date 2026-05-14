# FEAT-typst-pdf — Implementation Spec

**Ticket:** FEAT-typst-pdf
**Goal:** Replace the manual `lopdf`-based PDF generator (`src/pdf_gen.rs`) with a Typst-powered pipeline. The public API (`generate_pdf(json: String) -> Result<Vec<u8>, DocGenError>` exported via UniFFI) stays identical.

---

## Constraints (non-negotiable)

| Constraint | Detail |
|---|---|
| On-device only | No network calls, no package registry access, no system font discovery |
| Targets | `aarch64-linux-android`, `armv7-linux-androideabi`, `x86_64-linux-android`, plus standard desktop targets |
| Stack | Android default thread stack is 512 KB — Typst needs ≥ 8 MB; use `std::thread::Builder::stack_size` |
| Fonts bundled | Inter (Regular, Bold, Italic, BoldItalic) + JetBrains Mono (Regular, Bold) — SIL OFL, `include_bytes!` |
| Kept untouched | `src/docx_impl.rs`, `src/md_render.rs`, `src/highlight.rs`, `src/pdf_metrics.rs`, `src/pdf_width_tables.inc.rs` |
| lopdf retained | `extract_text()` still uses lopdf to read back PDF bytes; keep it in `Cargo.toml` |

---

## File Map

| File | Action | Responsibility |
|---|---|---|
| `Cargo.toml` | Modify | Add `typst`, `typst-pdf`, `time` dependencies |
| `assets/fonts/` | Create (directory) | Hold the 6 bundled `.ttf` font files |
| `src/typst_world.rs` | Create | `PocketCrewWorld` struct implementing `typst::World`; embeds fonts and serves image bytes via `file()` |
| `src/typst_gen.rs` | Create | `json_to_typst(doc: &Document) -> (String, Vec<(FileId, Bytes)>)`; full Block → Typst markup conversion plus collected image assets; also hosts `compile_to_pdf` |
| `src/lib.rs` | Modify | Replace `mod pdf_gen` with `mod typst_world; mod typst_gen`; rewire `generate_pdf` |
| `src/pdf_gen.rs` | Delete | Entire file removed after tests pass |

---

## Implementation Tasks

Tasks ordered TDD-first: write the failing test first, then implement just enough to make it green.

### Phase 0 — Feasibility spike (do FIRST, before any production code)

The Typst pipeline rests on two unverified assumptions: that `lopdf::Document::load_mem` + `extract_text` work on Typst-generated PDFs, and that the Typst dependency graph cross-compiles to `aarch64-linux-android`. If either fails, the plan changes shape. Verify both in a throwaway branch before Phase 1.

**Task 0.0 — lopdf compatibility spike**
In a scratch test (deleted after the spike), do:
1. Add `typst = "0.14.2"`, `typst-pdf = "0.14.2"` to `Cargo.toml` temporarily.
2. Write a one-off `#[test]` that builds a minimal `PocketCrewWorld` (Inter font only), compiles `= Hello\n\nThe quick brown fox.`, runs `typst_pdf::pdf`, then:
   - `lopdf::Document::load_mem(&bytes).expect("load")`
   - `doc.get_pages().len()` returns at least 1
   - `doc.extract_text(&[1])` returns a `String` containing both `Hello` and `quick brown fox` (no scrambling, no missing whitespace)
3. If `extract_text` fails or returns garbled text: switch the extraction backend in `extract_text()` (lib.rs:231) to `pdf-extract = "0.7"` BEFORE writing the production tests. Update Phase 4 accordingly.

**Task 0.1 — Android cross-compile spike**
Run `cargo ndk -t arm64-v8a -P 34 build` against the temporary Typst dependency. Confirm the dep graph (notably `comemo`, `usvg`, `ttf-parser`) builds cleanly. If a transitive dep needs feature gating, document the required `[features]` block in `Cargo.toml` now.

**Task 0.2 — Add font assets**
Download and place six font files under `assets/fonts/`:
- `Inter-Regular.ttf`
- `Inter-Bold.ttf`
- `Inter-Italic.ttf`
- `Inter-BoldItalic.ttf`
- `JetBrainsMono-Regular.ttf`
- `JetBrainsMono-Bold.ttf`

All are SIL OFL-licensed. Verify each file is a valid TTF (magic bytes `00 01 00 00` or `74 72 75 65`).

**Task 0.3 — Finalize Cargo.toml**
Make the spike's temporary additions permanent under `[dependencies]`:
```toml
typst     = "0.14.2"
typst-pdf = "0.14.2"
time      = { version = "0.3", default-features = false, features = ["std"] }
```
Do **not** add `typst-render`, `typst-svg`, `typst-html`, or `typst-kit`. The `time` crate is added solely to compute calendar Y/M/D for `World::today()` (see Task 1.2). Run `cargo check` to confirm the dependency graph resolves.

---

### Phase 1 — World implementation (TDD)

**Task 1.1 — Write failing tests for `PocketCrewWorld`**
In a new `src/typst_world.rs`, add a `#[cfg(test)]` module containing the W1–W7 scenarios from `test_spec.md`. Run `cargo test typst_world` — all must fail (file doesn't exist).

**Task 1.2 — Implement `PocketCrewWorld`**
Create `src/typst_world.rs` with:

```rust
use std::collections::HashMap;
use std::sync::Arc;
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, Source, VirtualPath};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, World};
use typst::diag::{FileError, FileResult};

static INTER_REGULAR:        &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
static INTER_BOLD:           &[u8] = include_bytes!("../assets/fonts/Inter-Bold.ttf");
static INTER_ITALIC:         &[u8] = include_bytes!("../assets/fonts/Inter-Italic.ttf");
static INTER_BOLD_ITALIC:    &[u8] = include_bytes!("../assets/fonts/Inter-BoldItalic.ttf");
static JETBRAINS_REGULAR:    &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");
static JETBRAINS_BOLD:       &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf");

const FONT_DATA: &[&[u8]] = &[
    INTER_REGULAR, INTER_BOLD, INTER_ITALIC, INTER_BOLD_ITALIC,
    JETBRAINS_REGULAR, JETBRAINS_BOLD,
];

pub struct PocketCrewWorld {
    source:  Source,
    main_id: FileId,
    library: LazyHash<Library>,
    book:    LazyHash<FontBook>,
    fonts:   Vec<Font>,
    files:   Arc<HashMap<FileId, Bytes>>, // image bytes addressed by synthetic VirtualPath
}
```

Implement `PocketCrewWorld::new(source_text: String, files: HashMap<FileId, Bytes>) -> Self`:
1. Iterate `FONT_DATA`. For each, call `Font::iter(Bytes::new(bytes))` and collect every `Font` into `fonts`.
2. Build `FontBook::from_fonts(fonts.iter())`.
3. Wrap with `LazyHash::new(book)` and `LazyHash::new(Library::builder().build())`.
4. `main_id = FileId::new(None, VirtualPath::new("/main.typ"))`.
5. `source = Source::new(main_id, source_text)`.
6. `files = Arc::new(files)`.

Implement `World` trait:
- `library(&self) -> &LazyHash<Library>` → `&self.library`
- `book(&self) -> &LazyHash<FontBook>` → `&self.book`
- `main(&self) -> FileId` → `self.main_id`
- `source(&self, id: FileId) -> FileResult<Source>` → if `id == self.main_id` return `Ok(self.source.clone())` else `Err(FileError::NotFound(PathBuf::from(id.vpath().as_rooted_path())))`
- `file(&self, id: FileId) -> FileResult<Bytes>` → lookup in `self.files`, return `Ok(bytes.clone())` on hit else `Err(FileError::NotFound(...))`
- `font(&self, index: usize) -> Option<Font>` → `self.fonts.get(index).cloned()`
- `today(&self, offset: Option<i64>) -> Option<Datetime>` → use the `time` crate: `time::OffsetDateTime::now_utc()`, apply `time::Duration::hours(offset.unwrap_or(0))`, then `Datetime::from_ymd(year, month as u8, day as u8)`. Return `None` if any conversion overflows.

`PocketCrewWorld` must be `Send + Sync`: `Arc<HashMap<FileId, Bytes>>` and all other fields are immutable post-construction.

**Task 1.3 — Verify Phase 1 tests pass**
`cargo test typst_world` must be green.

---

### Phase 2 — Typst markup generator (TDD)

**Task 2.1 — Write failing tests for `json_to_typst`**
Create `src/typst_gen.rs`. At the bottom, add the `#[cfg(test)]` module containing every G-series scenario from `test_spec.md` (G1–G43 inclusive). Provide the `doc_with_blocks` helper described there. Run `cargo test typst_gen` — all must fail.

**Task 2.2 — Define the generator signature and preamble**
Create:

```rust
use std::collections::HashMap;
use typst::foundations::Bytes;
use typst::syntax::{FileId, VirtualPath};

pub struct GeneratedSource {
    pub source: String,
    pub files:  HashMap<FileId, Bytes>,
}

pub fn json_to_typst(doc: &crate::Document) -> GeneratedSource { /* ... */ }
```

Emit the preamble (verbatim, before any block content):

```typst
#set page(margin: (x: 48pt, y: 56pt), numbering: "1")
#set text(font: "Inter", size: 12pt, fill: black)
#show raw: set text(font: "JetBrains Mono", size: 9pt)
#show raw.where(block: true): box.with(
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
#show heading: it => { v(0.5em); it; v(0.3em) }
#set list(marker: ([-], [--], [o]))
```

**Title rendering rule** (avoids duplicating a leading H1):
- Let `first_h1 = blocks.first().and_then(|b| if let Block::Heading { level: 1, text, .. } = b { Some(text) } else { None })`.
- If `first_h1 == Some(&metadata.title)`, skip metadata title emission (the block itself renders it).
- Otherwise emit `= {escape_typst(&metadata.title)}` followed by blank line.
- If `metadata.author` is present, emit `_By {escape_typst(author)}_` followed by blank line.

**Task 2.3 — Implement Block → Typst markup for all variants**

| Block variant | Typst output |
|---|---|
| `Heading { level, text, anchor }` | `"=".repeat(level)` + ` ` + `escape_typst(text)`; if `anchor` is `Some(a)`, append ` <{a}>` (Typst label). Blank line after. |
| `Paragraph { text, align }` | Inline markdown converted via `md_to_typst_inline(text)`. If `align == Some(Center)` or `Some(Right)`, wrap in `#align(center)[...]` / `#align(right)[...]`. Omit the wrapper for `None` and `Some(Left)` (left is default; redundant emit produces noise). Blank line after. |
| `Code { language, text, show_line_numbers, caption, .. }` | Use the **function form** `#raw(block: true, lang: "{lang}", "{rust-escaped-text}")` — never fenced markup. This avoids the closing-fence problem when content contains triple backticks. The text is escaped using `text.replace('\\', "\\\\").replace('"', "\\\"")` and wrapped in double quotes. If `show_line_numbers` is true: precompute the numbered text in Rust (`format!("{:>3} | {}\n", i+1, line)` per line) and emit that as the raw body instead of the original. If `caption` is `Some`, emit `#figure(caption: [{escape_typst(caption)}])[ ... ]` wrapping the raw call. Blank line after. |
| `List { ordered, items, start }` | For ordered with `start.is_some() && start != Some(1)`: emit the function form `#enum(start: {N}, [item1], [item2], ...)`. Otherwise use lightweight markup: `+ text` for ordered, `- text` for unordered. Nested children indent by two spaces per depth. For items with `checked: Some(true)`, prefix the item text with `#sym.checkmark.heavy + " "`; for `Some(false)`, prefix with `[☐] `. For `None`, no prefix. |
| `Table { header, rows, alignments, caption }` | `#table(columns: N, align: (a1, a2, ...), table.header([H1], [H2], ...), [R1C1], [R1C2], ...)`. Map `Alignment::Left → left`, `Center → center`, `Right → right` as bare Typst identifiers. If `alignments` is `None`, emit `align: left` (single value applies to all columns). Wrap the whole thing in `#figure(caption: [...])[ ... ]` if `caption` is `Some`. |
| `Image { src, alt, data_base64, mime, width_pt, height_pt, align }` | If `data_base64` is `Some`: decode base64 in Rust to `Vec<u8>`, allocate a fresh `FileId::new(None, VirtualPath::new(&format!("/img_{n}.{ext}")))` (where `ext` comes from `mime` mapped to "png"/"jpg"/etc, defaulting to "bin"), store `(FileId, Bytes::new(decoded))` in the `files` map of `GeneratedSource`, and emit `#image("/img_{n}.{ext}", width: {w}pt, height: {h}pt)`. Omit `width:`/`height:` when `None`. If `data_base64` is `None`: emit alt text as italic paragraph `_{escape_typst(alt.unwrap_or_default())}_` (or empty paragraph if `alt` is `None`). Counter `n` is generator-local. |
| `Math { display, latex }` | First detect LaTeX-only constructs via the regex `\\[A-Za-z]+` (any backslash followed by alpha). If a match is found, emit a Typst comment plus a raw fallback: `// unsupported LaTeX: {latex}\n#raw("{escaped_latex}")`. Otherwise emit `$ {latex} $` for display=true (with surrounding spaces, canonical form) or `$ {latex} $` inline-spliced into a paragraph for display=false. Document this limitation clearly in the public API docs. |
| `Blockquote { text, attribution }` | `#quote(block: true, attribution: [{escape_typst(attribution)}])[{md_to_typst_inline(text)}]` or `#quote(block: true)[...]` when attribution is `None`. |
| `HorizontalRule` | `#line(length: 100%)` + blank line. |
| `Callout { kind, title, text }` | `#block(fill: rgb("{COLOR}"), inset: 8pt, radius: 4pt, width: 100%)[*{KIND}{: title}*\n\n{md_to_typst_inline(text)}]`. Color map: Note `#BEE3F8`, Info `#BEE3F8`, Tip `#C6F6D5`, Warning `#FEFCBF`, Danger `#FED7D7`. KIND is the variant name in caps. Title segment omitted if `None`. |
| `Footnote { id, text }` | Emit `#footnote[{md_to_typst_inline(text)}]` at the point of declaration. The `id` field is currently unused — preserved in the schema for future ref-block work; do NOT track it in a side table. Document this in code comments next to the match arm. |
| `PageBreak` | `#pagebreak()` + blank line. |
| `Toc { max_depth }` | `#outline(depth: {N})` where `N = max_depth.unwrap_or(3)`. |

**Task 2.4 — Implement `md_to_typst_inline` helper**
Convert a Markdown inline string to Typst inline markup. Drive it from `pulldown-cmark` (already in deps). Walk events and emit:
- `**text**` (Strong) → `*text*`
- `*text*` (Emphasis) → `_text_`
- `~~text~~` (Strikethrough, requires `Options::ENABLE_STRIKETHROUGH`) → `#strike[text]`
- `` `code` `` (Code) → `` `code` ``
- `[label](url)` (Link) → `#link("url")[label]`
- Autolink `<url>` → `#link("url")`
- Hard line break (Event::HardBreak) → ` \ ` (Typst line break: space, backslash, space)
- Inline image `![alt](url)` → behaves like the `Image` block with no `data_base64`: emit `_alt_` (italic). Document that inline images via URL are not supported.
- Footnote reference `[^id]` → no-op (skip; the corresponding `Footnote` block carries the content)
- Plain text segments: pass through `escape_typst_inline` (see Task 2.5)
- Nested formatting (e.g., `**[link](url)**`) is handled naturally by walking events; emit Typst formatting wrappers around the recursive output

Markdown backslash escapes (`\*`, `\#`, etc.) appear as plain text in the pulldown stream; `escape_typst_inline` then re-escapes them appropriately for Typst.

**Task 2.5 — Implement escape helpers**
Two helpers, distinguished by context:

```rust
fn escape_typst(text: &str) -> String { /* plain text — heading text, attribution, etc. */ }
fn escape_typst_inline(text: &str) -> String { /* inline body inside [ ... ] content blocks */ }
```

Both escape: `#`, `<`, `>`, `@`, `_`, `*`, `$`, `\`, `` ` ``, `[`, `]` — each prefixed with `\`.

`escape_typst` additionally escapes line-start markup tokens to prevent unintended block parsing: after splitting on `\n`, if a line starts with any of `=`, `-`, `+`, `/` followed by space or end-of-line, prefix the first character with `\`. `escape_typst_inline` skips this line-start step because inline content blocks don't initiate block-level parsing.

**Task 2.6 — Verify Phase 2 tests pass**
`cargo test typst_gen` must be all green.

---

### Phase 3 — Compile pipeline and `generate_pdf` rewiring (TDD)

**Task 3.1 — Write failing integration tests**
Add to `src/lib.rs` a new `#[cfg(test)] mod typst_pdf_tests` containing every S, M, and E scenario from `test_spec.md`. Run `cargo test typst_pdf_tests` — all must fail (function still uses old impl).

**Task 3.2 — Add `compile_to_pdf` to `src/typst_gen.rs`**

```rust
use crate::typst_world::PocketCrewWorld;

pub fn compile_to_pdf(generated: GeneratedSource) -> Result<Vec<u8>, crate::DocGenError> {
    // Spawn dedicated thread with 8 MB stack (Android default is 512 KB; Typst layout is deeply recursive).
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let world = PocketCrewWorld::new(generated.source, generated.files);
            let warned = typst::compile::<typst::layout::PagedDocument>(&world);
            // Surface warnings via tracing so callers can opt in; do not fail on warnings.
            for diag in warned.warnings.iter() {
                tracing::warn!(target: "docgen.typst", "{}", diag.message);
            }
            let document = warned.output.map_err(|errors| {
                let msg = errors.iter().map(|e| e.message.to_string())
                    .collect::<Vec<_>>().join("; ");
                crate::DocGenError::GenerationError(msg)
            })?;
            typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default())
                .map_err(|errors| {
                    let msg = errors.iter().map(|e| e.message.to_string())
                        .collect::<Vec<_>>().join("; ");
                    crate::DocGenError::GenerationError(msg)
                })
        })
        .map_err(|e| crate::DocGenError::GenerationError(e.to_string()))?
        .join()
        .map_err(|_| crate::DocGenError::GenerationError("PDF thread panicked".to_string()))?
}
```

Notes:
- `typst::compile` returns `Warned<SourceResult<D>>`. Access the doc via `.output` (a `Result<D, EcoVec<SourceDiagnostic>>`).
- `typst_pdf::pdf` returns `SourceResult<Vec<u8>>`.
- If `tracing` is not already a dependency, replace the `tracing::warn!` line with a no-op or `eprintln!`-guarded-by-`debug_assertions`. Do NOT add `tracing` if absent.

**Task 3.3 — Rewire `generate_pdf` in `src/lib.rs`**

```rust
#[uniffi::export]
pub fn generate_pdf(json: String) -> Result<Vec<u8>, DocGenError> {
    let doc: Document = serde_json::from_str(&json)
        .map_err(|e| DocGenError::SerializationError(e.to_string()))?;
    let generated = typst_gen::json_to_typst(&doc);
    typst_gen::compile_to_pdf(generated)
}
```

Add `mod typst_world; mod typst_gen;` to the module list. Remove `mod pdf_gen;`.

**Task 3.4 — Verify Phase 3 tests pass**
`cargo test typst_pdf_tests` must be all green.

---

### Phase 4 — Test migration and cleanup

**Task 4.1 — Retire coordinate-checking PDF layout tests**
The `pdf_code_block_layout_tests` module tested lopdf content-stream operations and is now irrelevant. Delete the entire module from `src/lib.rs`. Delete `src/pdf_gen.rs` and its inline `code_wrap_tests` module.

**Task 4.2 — Update `test_pdf_paginates_long_content`**
Existing test uses `lopdf::Document::load_mem` for page counting. Confirm it still passes. If pagination differs from old code, adjust the paragraph count threshold.

**Task 4.3 — Update `test_generate_pdf_extract_has_no_markdown_asterisks`**
Confirm it passes; if not, the issue is in `md_to_typst_inline`. Fix there.

**Task 4.4 — Run full host test suite**
`cargo test --release` on host architecture. All surviving tests must pass:
- `highlight::tests::prepare_code_text_expands_tabs`
- `md_render::tests::*` (7 tests — untouched)
- Schema/validate tests (9 tests, untouched)
- `tests::test_generate_docx*` (3 tests)
- `tests::test_generate_pdf`, `test_pdf_paginates_long_content`, `test_generate_pdf_extract_has_no_markdown_asterisks`
- `tests::test_extract_text_unsupported`
- `typst_pdf_tests::*` (all S, M, E scenarios from test_spec.md)
- `typst_gen::tests::*` (all G scenarios)
- `typst_world::tests::*` (W1–W7)

**Task 4.5 — Android build smoke check**
Run `cargo ndk -t arm64-v8a -P 34 build --release` and confirm clean compile. Validates that `include_bytes!` paths resolve and the Typst dep graph supports Android target.

**Task 4.6 — Manual stack pressure check**
Compile a synthetic document with deeply nested structure (50 nested lists or 20 nested callouts) and confirm `generate_pdf` returns `Ok`. No automated test guards the 8 MB stack size; this manual check during sign-off catches a regression where someone replaces `Builder::stack_size` with `std::thread::spawn`.

---

## Key Design Decisions

### Why `std::thread::Builder::stack_size(8 * 1024 * 1024)`?
Typst's layout engine is deeply recursive. Android's default thread stack is 512 KB; desktop defaults are 2–8 MB. Spawning a dedicated 8 MB thread is the only portable way to guarantee enough stack on Android without modifying NDK linker flags.

### Why no system font discovery?
`typst-kit` provides `SystemFonts` but calls platform font APIs not available on Android. Bundling fonts via `include_bytes!` is simpler, fully reproducible, and adds ~3 MB to the binary (Inter ~1.8 MB, JetBrains Mono ~1.2 MB).

### Why serve images via `World::file()` instead of inline `bytes(...)`?
Typst's stdlib has no base64 decoder, and embedding raw decoded bytes inside the `.typ` source string is not portable (the source is UTF-8). Decoding base64 in Rust and exposing the bytes through `World::file(FileId)` is the only viable path. `json_to_typst` therefore returns both the source string AND a `HashMap<FileId, Bytes>` of registered images; `PocketCrewWorld::new` takes both.

### Why precompute line numbers in Rust instead of a Typst show rule?
A document-scoped show rule on `raw.where(block: true)` would apply to every subsequent code block, including ones that opted out. Local scoping with `#{ show ... ; raw(...) }` works but is fragile. Precomputing the numbered text gives byte-exact, testable output.

### Why use `#raw(...)` function form instead of fenced backticks?
Fenced code blocks (` ```rust ... ``` `) can be closed early if the body contains a triple backtick run. The function form `#raw(block: true, lang: "rust", "...")` takes the body as a Rust-escaped string literal, sidestepping the problem entirely.

### Why is LaTeX math treated as best-effort?
Typst's math syntax differs from LaTeX (`$\frac{a}{b}$` is invalid Typst). Without an on-device LaTeX→Typst converter (none exists that fits the no-network constraint), unsupported LaTeX commands are emitted as a Typst comment plus a raw-text fallback. Users wanting full LaTeX should pre-render math to images and use `Block::Image`.

### Why keep `lopdf` in `Cargo.toml`?
`extract_text()` reads back the PDF bytes produced by Typst. It still uses `lopdf::Document::load_mem`. The Phase 0 spike verifies lopdf can parse Typst output; if it cannot, swap to `pdf-extract`.

### Why keep `md_render.rs`?
`generate_docx` still uses `md_render::markdown_inline_to_spans` and `paragraph_from_markdown`. The Typst pipeline has its own `md_to_typst_inline` in `typst_gen.rs`.

### Why ignore `ListItem.checked` and `Block::Heading.anchor` in v0?
Both are preserved in the schema. Anchors emit a Typst label (`<anchor-name>`) suitable for future internal-link work. Checked items render as visible glyphs. Neither has cross-block linking semantics in this milestone.
