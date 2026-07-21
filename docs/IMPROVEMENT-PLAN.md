# perfect-print: Improvement Plan

> **Status:** Active. The plan tracks completed work and remaining known gaps.
> **Date:** 2026-06-14
> **Workspace:** `/Users/josephsee/Documents/NewApps/perfect-print/`

## Gap Analysis

### What the plan requires vs. current state

| # | Gap | Severity | Phase |
|---|-----|----------|-------|
| 1 | Right-align/justify not wired in paragraph engine | High | 3 |
| 2 | Font embedding missing in PDF backend | High | 2 |
| 3 | Error types are per-crate, no unified `PrintError` | Medium | 1 |
| 4 | Table row height uses char-count estimate, not shaped text | Medium | 3 |
| 5 | JSON roundtrip stability not tested with content | Medium | 1 |
| 6 | Style inheritance (document defaults → paragraphs) | Low | 3 |
| 7 | No hyphenation | Low | 3 |
| 8 | No fuzz testing | Low | 7 |
| 9 | Windows/Linux backends empty | Medium | 4-5 |
| 10 | No Tauri/egui/iced integrations | Medium | 6 |

## Prioritized Build Plan

### Item 1: Right-Align and Justify (HIGH)

**Problem:** `TextAlign` enum exists in `TextStyle` but the paragraph engine ignores it. All text renders at x=0 regardless of alignment setting.

**Approach:**
- In `ParagraphEngine::layout_line()`, after computing the line width, offset each glyph's x-position based on alignment:
  - `Left`: no change (x starts at 0)
  - `Right`: x starts at `max_width - line_width`
  - `Center`: x starts at `(max_width - line_width) / 2.0`
  - `Justified`: distribute extra space between words (not characters) proportionally. Skip for single-word lines and last line of paragraph.
- In `line_to_draw_command()` in flow.rs, pass the x-offset through to the `DrawCommand::Text` position.
- In the PDF backend's `render_command()` for `DrawCommand::Text`, the x-position is already set by the flow engine, so it should work automatically.
- In the raster backend, same — the position comes from the draw command.

**Verifiable end state:**
- `cargo test -p perfect-print-layout` includes tests:
  - `test_right_align_offsets_glyphs_right` — right-aligned line has first glyph at `max_width - line_width`
  - `test_center_align_centers_glyphs` — centered line has first glyph at `(max_width - line_width) / 2`
  - `test_justify_spreads_words` — justified paragraph with 3+ lines has increasing gaps between words
- `cargo run -p perfect-print-cli -- render hello --pdf /tmp/align_test.pdf` produces PDF where right-aligned text is visually right-aligned

### Item 2: Font Embedding in PDF (HIGH)

**Problem:** PDF backend references fonts by name (`/F1 Helvetica`) but doesn't embed font data. PDFs are non-portable.

**Approach:**
- Use `fontdb` to locate system fonts (already a dependency in layout crate)
- Read the raw `.ttf`/`.otf` file bytes from the filesystem
- For each font used in the document, create a PDF FontDescriptor with:
  - `/FontName` — the PostScript name from the font
  - `/FontFile2` — embedded TrueType stream (for `.ttf`)
  - `/FontFile3` — embedded CFF stream (for `.otf`)
  - `/Flags`, `/ItalicAngle`, `/Ascent`, `/Descent`, `/CapHeight`, `/StemV`
- Subset the font: only include glyphs actually used in the document
  - For simplicity, embed the full font first (subsetting is a v2 optimization)
- Add the font as a PDF stream object with `/Length1` (uncompressed length for TrueType)
- Reference the embedded font from the page's Resources dictionary

**Verifiable end state:**
- `cargo test -p perfect-print-pdf` includes `test_pdf_has_embedded_font` — parses PDF bytes, checks for `/FontFile2` or `/FontFile3` key
- `pdffonts /tmp/test_embedded.pdf` (if pdffonts is available) shows "yes" in the "emb" column
- PDF renders correctly on a system without Helvetica installed

### Item 3: Structured Error Types (MEDIUM)

**Problem:** Each crate has its own error type (`PdfError`, `CoreError`, `DialogError`). No unified error type with context chains. The plan requires `PrintError` implementing `std::error::Error` with file:line source location.

**Approach:**
- Create `perfect-print-core/src/error.rs` with a unified `PrintError` type:
  ```rust
  pub enum PrintError {
      Io { path: Option<PathBuf>, source: std::io::Error },
      Pdf { message: String, page: Option<usize> },
      Layout { message: String, element: Option<String> },
      Font { family: String, message: String },
      Image { id: String, message: String },
      Validation { message: String },
      Serialization { message: String },
  }
  ```
- Implement `std::error::Error`, `Display`, and `From` conversions for each variant
- Add `with_context(self, ctx: impl Display) -> Self` for chaining
- Replace `anyhow` usage in public APIs with `PrintError`
- Each crate's error type implements `Into<PrintError>`

**Verifiable end state:**
- `cargo test -p perfect-print-core` includes:
  - `test_error_display_includes_context` — `PrintError::Validation` displays the message
  - `test_error_from_io` — `std::io::Error` converts to `PrintError::Io`
  - `test_error_chain` — `.with_context("while loading font")` produces chained message
- `cargo doc --workspace` builds without warnings

### Item 4: Table Cell Content Measurement (MEDIUM)

**Problem:** `TableEngine::calculate_row_height()` estimates height from character count (`text.len() * size * 0.5 / width`). This is wrong for proportional fonts, mixed content, and non-Latin text.

**Approach:**
- Shape the cell text using the `TextShaper` (already available in the layout crate)
- Use actual glyph advances to compute the number of lines that fit in the cell width
- Compute row height from the shaped line count × line height
- For `CellContent::Commands`, keep the current 20.0pt estimate (commands are pre-positioned)

**Verifiable end state:**
- `cargo test -p perfect-print-layout` includes:
  - `test_table_row_height_uses_shaped_text` — a cell with "Hello World" at 12pt in a 100pt-wide column produces a specific, predictable height
  - `test_table_row_height_wraps_long_text` — text that doesn't fit in one line produces a taller row
  - `test_table_row_height_empty_cell` — empty cell produces minimum height (padding × 2 + line_height)

### Item 5: JSON Roundtrip Stability (MEDIUM)

**Problem:** `DocumentModel` derives `Serialize`/`Deserialize` but there are no tests proving that serialization is stable across runs and that deserialization produces an identical model.

**Approach:**
- Add tests to `perfect-print-core/src/document.rs`:
  - Build a document with text, images, headers, footers, multiple pages
  - Serialize to JSON, deserialize back, serialize again
  - Assert `json1 == json2` (byte-identical)
  - Assert `model1 == model2` (structurally equal)
- Ensure `serde_json::to_string_pretty` is used consistently (it sorts keys by default in recent serde versions)

**Verifiable end state:**
- `cargo test -p perfect-print-core` includes `test_json_roundtrip_stability` — passes with a multi-page document containing text, images, headers, and footers

### Item 6: Style Inheritance (LOW)

**Problem:** Each `ContentBlock::Paragraph` carries its own `TextStyle`. There's no document-level default that paragraphs inherit.

**Approach:**
- Add `default_style: TextStyle` to `FlowConfig`
- In `FlowLayoutEngine::layout()`, when processing a `ContentBlock::Paragraph`, merge the paragraph's style with the document default (paragraph fields override defaults)
- Add `DocumentBuilder::default_style()` method

**Verifiable end state:**
- `cargo test -p perfect-print-layout` includes `test_style_inheritance` — a paragraph with no explicit font inherits the document default font

## Execution Order

1. Right-align and justify (most visible impact)
2. Font embedding (correctness)
3. Structured errors (foundation for strictness modes)
4. Table cell measurement (reliability)
5. JSON roundtrip (easy win)
6. Style inheritance (polish)

## Current Status

Core build/test status should be verified with `cargo test --workspace`. Several earlier items are implemented, but the project is not yet a finished full-featured native print API.

| # | Item | Status | Tests Added |
|---|------|--------|-------------|
| 1 | Right-align and justify | Done | 3 (paragraph alignment) |
| 2 | Font embedding in PDF | Done | 1 (pdf_has_embedded_font) |
| 3 | Structured error types | Done | 7 (PrintError, PrintWarning, Strictness, ValidationResult) |
| 4 | Table cell measurement | Done | 3 (row height tests) |
| 5 | JSON roundtrip stability | Done | 1 (roundtrip test) |
| 6 | Style inheritance | Done | 2 (rich/plain paragraph default-style inheritance) |

### Verifiable end states achieved:
- **Alignment**: Right-aligned text has glyphs positioned at `max_width - line_width`. Center-aligned glyphs are centered. Justified text distributes extra space between words.
- **Font embedding**: `pdf_has_embedded_font` test verifies `/FontFile2` and `/FontDescriptor` in PDF output. PDFs embed actual TrueType font data.
- **Error types**: `PrintError` implements `std::error::Error` with `thiserror`, has `with_context()` for chaining, `is_not_found()` / `is_validation()` helpers. `ValidationResult` supports `Strictness::BestEffort/Warn/Exact`.
- **Table measurement**: `TableEngine` uses `TextShaper` + `FontCache` for actual glyph width measurement instead of char-count estimates.
- **JSON roundtrip**: `DocumentModel` serializes to JSON, deserializes back, and produces stable JSON at the model layer. The public `Document::from_json()` path now preserves pages and commands instead of returning an empty document.
- **Style inheritance**: `FlowConfig.default_style` (set via `Document::default_style()`) is merged into every `Paragraph` and `RichParagraph` (including each of its spans) via `merge_styles()` in `flow.rs`: unset fields (empty font, zero size, default black color, default left alignment) fall back to the document default; explicitly-set fields win. `test_paragraph_inherits_flow_default_style` and `test_rich_paragraph_inherits_flow_default_style` in `crates/perfect-print-layout/src/flow.rs` assert the merged font/size/color reach the rendered `DrawCommand::Text` runs, not just that layout succeeds.

- Hyphenation (requires a hyphenation dictionary)
- Windows/Linux backends (requires platform-specific work)
- Tauri/egui/iced integrations (requires GUI framework knowledge)
- Barcodes/QR codes (separate feature)
- WASM target (requires `wasm32` testing)

## 2026-07-21: HTML/CSS compatibility + rich text/list API

Implemented per `docs/superpowers/plans/2026-07-21-html-css-compatibility.md`.

- **Rich-text spans** (Task 1): `ContentBlock::RichParagraph` carries mixed-style
  `StyledSpan`s (bold/italic/underline/strikethrough/color per span, sharing one
  baseline per line); public `RichParagraph` builder in `perfect-print`.
- **List blocks** (Task 2): `ContentBlock::List` (`ListKind::Bulleted`/`Numbered`,
  nested `level`); public `List` builder (`List::bulleted()/numbered()`, `.item()`,
  `.rich_item()`, `.nested()`).
- **Style inheritance** (Task 3): already wired and verified — see item 6 above
  (`FlowConfig.default_style` merges into `Paragraph`/`RichParagraph`).
- **CSS subset parser** (`perfect-print-html::css`, `::stylesheet`, Task 4):
  hand-rolled declaration/length/color parsing, selector cascade with
  id > class > tag specificity, `@page` extraction.
- **HTML/CSS pipeline** (`perfect-print-html::convert`, Task 5): `scraper`-based
  DOM walk with cascade resolution, lowering into the same `ContentBlock`s the
  native `Document` builder produces — no second rendering path.
- **`HtmlDocument::render()`** (Task 6): validate → parse/cascade/convert → flow
  layout → `DocumentModel`, with `HtmlRenderResult::to_pdf_bytes()`/`save_pdf()`/
  `render_png()`. Page-setup precedence: explicit `page_settings()` >
  `@page` > letter default. Title precedence: explicit `.title()` >
  HTML `<title>`. `ReadinessTracker` was simplified from an async
  load/timeout model (inherited from a WebView-era design) to a plain
  stage tracker matching the synchronous pure-Rust pipeline.
- **CLI `render-html` subcommand** (Task 7): `perfect-print-cli render-html
  <input.html> [--pdf] [--png-dir] [--dpi] [--base-dir] [--strict]`.
- **Bold/italic rendering bug fix** (found while verifying Task 7's demo
  output): `TextStyle.bold`/`.italic` were never consulted when selecting a
  font face for shaping (`perfect-print-layout`), rasterizing
  (`perfect-print-render`), or embedding (`perfect-print-pdf`) — every run
  used the regular face. `perfect-print-render`'s raster font cache also
  discarded the font-collection face index from `fontdb` and always parsed
  face 0 (Regular) of any TrueType Collection, which was the root cause once
  the family/weight/style query itself was fixed. All three layers now
  select/key by family + bold + italic.

New crate: `perfect-print-html` (`scraper`, `ego-tree`, `url` deps). New docs:
`docs/html-css-support.md`. New fuzz target: `fuzz/fuzz_targets/fuzz_html_convert.rs`.

- **Page margins bug fix**: `FlowLayoutEngine` laid out every block in
  content-area-relative coordinates (x/y start at 0 inside the margins), but
  `build_document()` only stored `page.margins` as metadata — neither the
  raster renderer nor the PDF backend ever read it, so every flow-laid-out
  document (including all HTML/`@page { margin: ... }` output) rendered flush
  against the top-left page corner regardless of configured margins.
  `build_document()` now translates every emitted `DrawCommand` (added
  `DrawCommand::translated()`/`Point::translated()`/`Rect::translated()`/
  `PathOp::translated()` in `perfect-print-core`) by `(margins.left,
  margins.top)`, so the canonical `DocumentModel` carries page-absolute
  coordinates that every backend can consume directly.
  `ContentBlock::Commands` blocks (e.g. the HTML `<hr>` rule in
  `perfect-print-html::convert`) are authored in the same content-relative
  space as everything else, so they're translated identically — verified by
  `test_commands_block_is_offset_by_margins` alongside
  `test_layout_offsets_content_by_margins` and
  `test_zero_margins_content_at_origin` in `crates/perfect-print-layout/src/flow.rs`.
