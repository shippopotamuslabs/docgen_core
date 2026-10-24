# docgen_core (Rust)

Document generation for Enclave, as a Rust crate with a UniFFI bridge to
Kotlin. It takes a JSON document, a versioned block model of headings,
paragraphs, lists, tables, code, and math, and turns it into finished files:

- `generate_docx(json)` renders a Word document.
- `generate_pdf(json)` renders a PDF through Typst.
- `extract_text(bytes, mime_type)` pulls text back out of PDF, DOCX, and
  spreadsheet files.
- `get_schema()` and `validate_document(json)` expose the document model's
  JSON Schema and check a payload against it.
- A second, artifact-shaped API (`get_start_artifact_schema`,
  `get_append_blocks_schema`, and their validators) serves the app's
  start-then-append writing flow.

Supporting pieces: LaTeX-to-Typst conversion for math, math rendered to PNG,
Markdown rendering, syntax highlighting, and repair passes that fix common
JSON payload damage before validation (`json_repair`, `payload_heal`,
`serde_coerce`). Bundled fonts live in `assets/fonts/` (Inter, JetBrains
Mono, Noto Color Emoji) so PDF output looks the same on every device.

The Kotlin bindings package is `com.browntowndev.enclave.core.docgen`
(`uniffi.toml`).

## Build and test (host)

```bash
cargo build
cargo test
```

## Android build

```bash
./build-android.sh
```

The script builds the crate for `arm64-v8a` (Android API 34) with cargo-ndk
and runs uniffi-bindgen to write the Kotlin bindings and the `.so` into the
host app's tree.

Cold clone note: `build-android.sh` resolves paths two levels up from this
repo, into a `core/docgen` module of the Enclave app. It expects this crate
to sit inside that layout. From a standalone clone you can build and test
the Rust crate itself; the Android packaging step needs the Enclave tree
around it, plus cargo-ndk and the Android NDK on the machine.

## Layout

| Path | Contents |
| --- | --- |
| `src/lib.rs` | The exported UniFFI surface and the document model |
| `src/docx_impl.rs`, `src/typst_gen.rs`, `src/typst_world.rs` | The DOCX and Typst/PDF renderers |
| `src/md_render.rs`, `src/highlight.rs`, `src/latex_to_typst.rs`, `src/math_png.rs` | Markdown, code highlighting, and math handling |
| `src/json_repair.rs`, `src/payload_heal.rs`, `src/serde_coerce.rs` | Payload cleanup ahead of validation |
| `plans/` | The spec and test spec for the Typst PDF feature (`FEAT-typst-pdf`) |
