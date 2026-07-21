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

- **PDF font embedding: TrueType Collection gap closed.** `embed_truetype_font`
  in `crates/perfect-print-pdf/src/lib.rs` used to write the *entire* raw
  font-loader bytes into the `/FontFile2` stream. On macOS, `fontdb` usually
  resolves system fonts (e.g. Helvetica) from a `.ttc` TrueType Collection
  file, so the embedded stream was the whole multi-face collection rather
  than the single face `/FontDescriptor` declared — bloating PDFs and
  producing an embedded font program strict PDF viewers can't render text
  from. The face index was already used correctly for metrics/outlines
  (`font_loader.rs`, raster `FontCache`); only the embedded bytes were wrong.
  New module `crates/perfect-print-pdf/src/sfnt.rs` adds
  `extract_ttc_face(data, face_index)`, a pure, panic-free function that
  detects the `ttcf` magic, parses the referenced face's sfnt offset table,
  and rebuilds a standalone sfnt (table directory rewritten, tables 4-byte
  aligned/zero-padded, per-table and whole-font checksums recomputed,
  `head.checkSumAdjustment` recomputed per the OpenType spec). Non-TTC input
  passes through unchanged; malformed/truncated TTCs fail gracefully
  (`log::warn!` + fall back to embedding the original bytes) rather than
  panicking. Verified end-to-end with `render-html` on a real
  Helvetica.ttc-backed document: output PDF shrank from 7,112,685 bytes to
  1,534,755 bytes for the same content, with `pdffonts` still reporting all
  three faces (Regular/Bold/Italic) as embedded (`emb yes`).

## 2026-07-21: PlainBooks blank-page print fix (two root causes)

Two independent bugs, together the root cause of PlainBooks invoices
printing as blank pages on macOS. Fixed on `fix/print-blank-pages`.

- **PDF font dictionaries were spec-invalid (missing `/FirstChar`,
  `/LastChar`, `/Widths`).** ISO 32000-1 §9.6.2 requires all three on a
  simple TrueType font dictionary; `pdf_embedded_font` in
  `crates/perfect-print-pdf/src/lib.rs` wrote none of them. CoreGraphics logs
  `missing or invalid 'FirstChar' entry` (`CG_PDF_VERBOSE=1`) and falls back
  to guessing widths from the embedded font program; strict CUPS/driver
  PDF→PostScript filters in real print pipelines go further and drop the
  offending text run entirely, producing blank pages. `embed_truetype_font`
  now computes a `/Widths` array (1000-units-per-em) for the full WinAnsi
  32..=255 code range from the same `ttf_parser::Face` used for
  `/FontFile2` (mapping each WinAnsi code to Unicode via a table — the
  0x80-0x9F block is CP1252, not Latin-1 — then to a glyph advance, 0 for
  unmapped/missing glyphs), and `pdf_embedded_font` writes `/FirstChar 32
  /LastChar 255 /Widths [...]`; `FontDescriptor` also gets `/MissingWidth
  0`. Fixing this exposed a second, previously-masked bug in
  `build_tj_array`'s `TJ`-array adjustment math (it assumed every glyph's
  reader-applied default advance was 0, and used the *next* character's
  shaped advance instead of the *previous* one's) — both are fixed together
  since a correct per-glyph `TJ` adjustment requires `declared_width -
  shaped_advance`. New regression test
  `tests::pdf_font_dict_has_valid_widths_array` renders "Hi", parses the
  output PDF, and asserts the dictionary and width values are correct.
- **macOS print view double-applied the imageable-area offset, clipping
  content.** `PerfectPrintPDFView drawRect:` in
  `crates/perfect-print-backend-macos/src/native_print.m` manually
  translated by `NSMinX(imageable) + (NSWidth(imageable) - renderedWidth) /
  2.0` (and the y equivalent) — but AppKit already maps the rect
  `rectForPage:` returns onto the paper's imageable area itself, including
  `horizontallyCentered`/`verticallyCentered` placement, before calling
  `drawRect:`. The result was two stacked offsets; with Letter media on A4
  paper at Custom(1.0) scale this translated content to x ≈ −8.4pt,
  clipping the left ~8pt of every printed page (first character partially
  cut off). `rectForPage:` and `drawRect:` now share one `scaleForMedia:`
  helper so their scale factors can't drift, `drawRect:` only scales the CTM
  and translates the PDF's own MediaBox origin to view-space (0,0) — no
  imageable-origin translation, no centering math, since AppKit already
  applies both. The view's init-time frame is now sized conservatively (max
  media size across all pages, times `max(1.0, custom_scale)` for Custom
  mode) so every `rectForPage:` rect fits within the view's bounds as
  AppKit requires. Verified hermetically (no printer/dialog) with an
  `NSPrintOperation` + `NSPrintSaveJob` harness reproducing the view
  verbatim: before the fix, mode 3 (Custom, scale 1.0) with Letter media on
  A4 paper translated content by `(-8.4, +24.9)`pt; after the fix the
  translate term is gone entirely and rasterized output shows no clipping
  at any edge across modes 0 (FitToPage), 2 (None), and 3 (Custom).

## 2026-07-21: physical CSS units + `position: absolute` for template fidelity

Root cause: PlainBooks generates absolutely positioned HTML
(`<div style="position:absolute;left:0.5in;top:1.2in;...">`) with physical
units, but `perfect-print-html` supported neither `position: absolute` nor
`in`/`cm`/`mm` units — every template element collapsed into a top-down text
flow, ignoring its authored coordinates. Fixed on
`feature/absolute-positioning`.

- **Physical length units.** `css::parse_length` gained `in` (×72), `cm`
  (×72/2.54), `mm` (×72/25.4), and `pc` (×12), alongside the existing
  `pt`/`px`/`em`/bare-number. Since `@page { size: ... }` and `left`/`top`/
  `width` all route through this same function, `@page { size: 8.5in 11in }`
  now resolves end-to-end to a 612×792pt page with no separate plumbing.
- **`position: absolute`.** New `ContentBlock::Positioned { x, y, width,
  blocks }` primitive in `perfect-print-layout/src/flow.rs`: laid out
  outside the normal flow, on the current page, translated to `(x, y)`,
  without moving the flow cursor. Content taller than the remaining page
  overflows past the edge rather than paginating or clipping — matching
  real CSS out-of-flow semantics. `FlowLayoutEngine::layout()` was
  refactored to route through a new `layout_into_pages(blocks,
  content_width, page_height)` helper so `Positioned` content can recurse
  into it with `page_height = f64::INFINITY` (guaranteeing every block
  takes the "fits on this page" branch) without touching
  `paragraph.rs`/`text_shaper.rs` (off-limits — another session had them
  checked out). In `perfect-print-html/src/convert.rs`, a `div` with
  `position: absolute` now converts to a `Positioned` block (`left`/`top`
  default to 0, `width` defaults to the remaining content width from `x`);
  `position: relative` is accepted as a flow-preserving no-op.
- **Verification:** rendered a representative PlainBooks invoice template
  (`/tmp/pb-invoice.html` — 8.5×11in `@page`, absolutely positioned title,
  address block, table, and right-aligned total) via `perfect-print-cli
  render-html --dpi 100` and visually confirmed the output PNG matches the
  authored layout: INVOICE title top-left at ~0.5in, address block below
  it, items table spanning most of the width in the middle, and the total
  right-aligned near the bottom-right (~7in down of an 11in page) — not a
  stacked top-down flow.
- Baseline `cargo test --workspace` (121 tests in the `perfect-print` crate
  plus all other crates, one observed flake in
  `adversarial_render_png_high_dpi` unrelated to this work) was recorded
  before any change; the final `cargo test --workspace` run after both
  features shows no new failures.

### Follow-up (same day): image sizing inside `position: absolute` boxes

Root cause: `position: absolute` shipped above gave templates working
`left`/`top`/`width`, but `<img>` sizing (`convert.rs::emit_img`) never
looked at CSS at all — it only read the (non-CSS) HTML `width`/`height`
attributes, falling back to the image's natural pixel dimensions. PlainBooks
invoice templates emit `<div style="position:absolute;...;width:Wpt;
height:Hpt;overflow:hidden"><img style="width:100%;height:100%;
object-fit:contain" src="data:image/png;base64,..."/></div>` — a `%` CSS
size the old code couldn't parse, so it silently fell through to natural
size. A multi-thousand-pixel logo then rendered far larger than its
template box, opaque, covering every earlier-drawn element on the page (a
real print artifact showed the extracted text was all present via
`pdftotext`, but the raster showed only the giant logo — confirming a
render-time layout bug, not a content-loss bug).

Fixed (still `feature/absolute-positioning`'s follow-on work, merged to
`main`):

- **`height` is now a parsed property** (`apply_declarations`), stored on
  `ElementProps::explicit_height` — previously it hit the
  `unsupported CSS property` catch-all. Currently only acted on by
  `position: absolute` containers, to establish a `BoxContext` (width +
  optional height) for percentage resolution; on other elements it's parsed
  (no warning) but has no layout effect yet, since block heights are
  content-driven elsewhere in this renderer.
- **`width`/`height` accept `%`** (`ElementProps::width_percent` /
  `height_percent`), resolved only for `<img>`, only against the nearest
  enclosing positioned container's `BoxContext`. No container, or a
  container with unresolved height for a `%` height: falls through to the
  next rule below rather than warning or panicking.
- **`object-fit: contain` / `fill`** parsed (`ElementProps::object_fit`);
  `fill` (the CSS default, and the fallback when unspecified) stretches to
  the resolved box, `contain` scales the natural image to fit inside it
  preserving aspect ratio.
- **`Converter::emit_img` rewritten** as `resolve_image_size`, a strict
  precedence chain: (1) CSS `width`/`height` (absolute or `%`, with
  `object-fit` applied when both resolve, aspect-derived when only one
  does), (2) legacy HTML `width`/`height` attributes, (3) — the actual bug
  fix — no resolvable CSS size but inside a positioned container with a
  known box: fit to the box (contain semantics, never upscaling) instead of
  natural size, (4) no container either: cap at the remaining content
  width, preserving aspect ratio. A print page can now never render an
  image larger than its declared template slot, or, absent one, larger
  than the page.
- **Tests** (`crates/perfect-print-html/src/convert.rs`, TDD — written
  failing first): a synthetic 20×10 PNG inside a 144×72pt positioned box
  with `width:100%;height:100%;object-fit:contain` fits the box exactly
  (matching aspect ratio); the same image with no CSS size at all inside a
  50×50pt box is capped to the box, not rendered at its natural 20×10px;
  `width:100%` with no enclosing positioned container falls back to the
  content-width cap instead of panicking; a synthetic 2000×1000px image
  with no styles anywhere is capped at the 468pt default content width,
  aspect preserved. `cargo test -p perfect-print-html` and
  `cargo test --workspace` both green (161 total across the workspace, no
  regressions).
- `docs/html-css-support.md` gained an "Image sizing" section documenting
  the precedence chain, and `%`/`height`/`object-fit` are now listed as
  supported properties.

## 2026-07-21: `white-space: pre-wrap` / `pre-line` support

Root cause: PlainBooks emits customer/address blocks as
`<div style="...;white-space:pre-wrap;">Russ Johnson\n24604 Blue Goose
Rd\nBokoshe, OK</div>` — literal `\n` characters, no `<br>`. `white-space`
hit the `apply_declarations` catch-all (`unsupported CSS property:
white-space`), so the property had no effect, and the newlines fell into
`collapse_whitespace`'s normal-HTML rule (any run of `[\t\n\r ]` → one
space), collapsing a three-line address into a single run-on line on the
printed page.

Fixed (TDD, failing tests written first):

- **`perfect_print_core::draw::WhiteSpace`** — new inherited enum
  (`Normal` (default) / `PreWrap` / `PreLine`) added to `TextStyle`, since
  `white-space` inherits down the DOM the same way font/color/align do and
  `TextStyle` is already the vehicle `apply_declarations`/`resolve` thread
  for inherited properties. Only the HTML converter consults it — layout
  and rendering carry the field but ignore it, so it's a no-op everywhere
  else in the workspace. `merge_styles` (`perfect-print-layout/src/flow.rs`)
  updated to pass it through (paragraph style wins, matching every other
  field in that merge).
- **`apply_declarations`** gained a `"white-space"` arm parsing
  `normal`/`pre-wrap`/`pre-line`; other values (`nowrap`, `pre`,
  `break-spaces`, ...) still warn via `unsupported white-space: {value}`
  (the property is removed from the generic catch-all only for the values
  actually handled).
- **`Converter::collect_inline_node`**: when the active style's
  `white_space` is not `Normal`, a text node's literal `\n` characters are
  now split out and replaced with the same `BR_MARKER` spans `<br>` already
  uses, before whitespace collapsing runs — so `split_on_br` and
  `collapse_span_whitespace` (unchanged) handle pre-wrap/pre-line text
  exactly like hand-authored `<br>` markup: each line becomes its own
  `RichParagraph`, and remaining interior space/tab runs still collapse to
  one space per line (`pre-line` semantics). `pre-wrap` is treated
  identically to `pre-line` — see the "Simplification" note in
  `docs/html-css-support.md`'s new `white-space` section for why (no
  whitespace-preserving text run type exists in the layout engine, and
  nothing in the observed PlainBooks templates needs literal space runs
  preserved).
- Under the default `normal`, behavior is byte-for-byte unchanged — a
  `\n` still collapses to a single space.
- **Tests** (`crates/perfect-print-html/src/convert.rs`): a
  `position:absolute` div with `white-space:pre-wrap` and text `A\nB\nC`
  produces three separate `RichParagraph` blocks (mirroring how the
  existing `hr_becomes_rule_and_br_breaks` test asserts `<br>` splits into
  separate paragraphs); `white-space:pre-line` with `"A\n   B"` collapses
  the interior run to two clean lines (`"A"`, `"B"`); a plain `<div>A\nB</div>`
  with no `white-space` set still collapses to `"A B"`. `cargo test -p
  perfect-print-html` and `cargo test --workspace` both green, no
  regressions.
- `docs/html-css-support.md`: `white-space` added to the supported CSS
  properties list, with a new `## white-space` section documenting the
  `pre-wrap`≈`pre-line` simplification.

## 2026-07-21: `@page` margin shorthand (per-side margins)

Root cause (found while fixing PlainBooks print margins): `@page { margin:
... }` only ever accepted a single length via `parse_length`, mapped onto
`Margins::all(margin)` — a bare number, never CSS's 1/2/3/4-value margin
shorthand or the `margin-top`/etc. longhands. PlainBooks' native-print HTML
path wanted to emit its per-side template margins (default 0.5in each, but
independently configurable) into `@page`, and a 4-value shorthand would have
silently failed to parse (the whole multi-token string doesn't match any
single unit suffix), falling back to `PageSetup::default()`'s 1-inch
`Margins::all(72.0)` — a different, wrong margin, not the zero margin
PlainBooks previously had, but still not what the template specified.

Fixed:

- **`stylesheet.rs::PageRule.margin`** changed from `Option<f64>` to
  `Option<perfect_print_core::page::Margins>`.
- **`parse_margin_shorthand`** (new, `stylesheet.rs`) parses the standard
  CSS 1/2/3/4-value `margin` shorthand (each token run through the existing
  `parse_length`), mirroring `margin: top right bottom left` semantics
  (2-value = vertical/horizontal, 3-value = top/horizontal/bottom).
- **`margin-top`/`margin-right`/`margin-bottom`/`margin-left`** are now
  parsed as individual `@page` properties too (previously fell into the
  generic `unsupported @page property` warning), cascading over a preceding
  `margin` shorthand so a longhand after the shorthand overrides just that
  side — normal CSS cascade behavior.
- **`convert.rs::resolve_page_setup`** now assigns `rule.margin` directly
  onto `PageSetup.margins` (previously wrapped it in `Margins::all`).
- **Tests** (`stylesheet.rs`): the existing `at_page_rule_extracted` test
  updated for the new `Option<Margins>` type; two new tests cover the
  2/3/4-value shorthand forms and a longhand (`margin-left`) overriding one
  side of a preceding shorthand. `cargo test -p perfect-print-html` and
  `cargo test --workspace` both green.
- `docs/html-css-support.md`'s `@page` entry rewritten to document the
  shorthand forms and longhand cascade.
