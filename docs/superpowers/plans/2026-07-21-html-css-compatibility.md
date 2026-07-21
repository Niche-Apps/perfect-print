# HTML/CSS Compatibility + API Improvements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `perfect-print-html` actually render HTML/CSS to the canonical document model (and thus PDF/PNG/print), and land the core-API improvements that HTML requires: inline rich-text spans, list blocks, and verified style inheritance.

**Architecture:** Pure-Rust pipeline — no WebView. `HtmlDocument` → parse HTML with `scraper` (html5ever) → resolve a CSS subset (inline `style=`, `<style>` blocks with tag/class/id selectors, cascade with inheritance) → convert the styled DOM into layout `ContentBlock`s → reuse the existing `FlowLayoutEngine` → `DocumentModel` → existing PDF/raster/print backends. This keeps output deterministic and CI-testable, which is the project's core value (the existing `HtmlRenderStage::CreateWebView` variant is renamed accordingly).

**Tech Stack:** Rust workspace; new deps for `perfect-print-html`: `scraper = "0.20"` (HTML parsing + selector matching), existing `perfect-print-core`, `perfect-print-layout`, `perfect-print` crates. CSS declaration parsing is hand-rolled (property:value pairs are simple and this avoids a heavyweight dependency).

**Working branch:** create `feature/html-css` off the current branch before Task 1. Commit after every task.

**Baseline check before starting:** `cargo test --workspace` must pass (or note pre-existing failures and do not make them worse).

---

## Supported HTML/CSS subset (the contract)

Block elements: `h1`–`h6`, `p`, `div`, `blockquote`, `br`, `hr`, `ul`, `ol`, `li`, `table`/`thead`/`tbody`/`tr`/`td`/`th`, `img`.
Inline elements: `b`/`strong`, `i`/`em`, `u`, `s`/`strike`/`del`, `span`, `a` (rendered as styled text, no link annotation yet), `code`.
Ignored (with warning collected, not error): `script`, `style` (consumed as CSS), `head` metadata except `<title>`, unknown tags treated as `div` (block) or `span` (inline) by display default.

CSS properties: `font-family`, `font-size` (`pt`, `px` at 96dpi→points ×0.75, `em` relative to parent, bare number = pt), `font-weight` (`normal`/`bold`/100–900), `font-style`, `color` (`#rgb`, `#rrggbb`, `rgb(r,g,b)`, 16 named CSS colors), `text-align` (`left|right|center|justify`), `line-height` (unitless multiplier or length), `text-decoration` (`underline`, `line-through`), `margin-top`/`margin-bottom` (block spacing → `Gap`), `letter-spacing`, `background-color` (table cells only), `padding` (table cells only), `page-break-before: always`/`page-break-after: always`, `break-before: page`/`break-after: page`.
Selectors: `tag`, `.class`, `#id`, `tag.class`, and comma lists. Specificity: id(100) > class(10) > tag(1), later rule wins ties. Inline `style=""` beats all.
`@page { size: ...; margin: ... }`: `size: letter|a4|legal|<w> <h>` and margin lengths map to `HtmlPageSettings` + document margins.

Default UA stylesheet (hard-coded): `h1` 24pt bold, `h2` 18pt bold, `h3` 14pt bold, `h4`–`h6` 12pt bold, body/p 12pt Helvetica black, `blockquote` left-indent 36pt, `code` Courier, margins between blocks = 0.5 × font-size top and bottom (h-tags 0.75×).

Everything outside the subset must degrade gracefully: unknown properties/values are collected as warnings on the conversion result, never a hard error (mirrors the project's `Strictness::Warn` philosophy).

---

### Task 1: Rich-text spans in the layout engine (core API improvement)

The HTML converter needs paragraphs with mixed inline styles (`Hello <b>world</b>`). Today `ContentBlock::Paragraph { text, style }` is single-style.

**Files:**
- Modify: `crates/perfect-print-layout/src/flow.rs` (add `ContentBlock::RichParagraph`)
- Modify: `crates/perfect-print-layout/src/paragraph.rs` (span-aware layout)
- Modify: `crates/perfect-print/src/lib.rs` (public `RichParagraph` builder)
- Test: inline `#[cfg(test)]` tests in `flow.rs`/`paragraph.rs`

- [ ] **Step 1: Write the failing test** in `crates/perfect-print-layout/src/flow.rs` tests module:

```rust
#[test]
fn test_rich_paragraph_layout_produces_runs_per_span() {
    let base = TextStyle::new(FontRef::new("Helvetica"), 12.0);
    let mut bold = base.clone();
    bold.bold = true;
    let block = ContentBlock::RichParagraph {
        spans: vec![
            StyledSpan { text: "Hello ".into(), style: base.clone() },
            StyledSpan { text: "world".into(), style: bold },
        ],
        base_style: base,
    };
    let mut engine = FlowLayoutEngine::new(FlowConfig::default());
    let model = engine.layout(&[block]);
    // Both spans appear, bold span carries bold=true, and the bold run
    // starts to the right of the plain run on the same baseline.
    let texts: Vec<_> = model
        .all_commands()
        .filter_map(|c| match c {
            DrawCommand::Text(run) => Some(run),
            _ => None,
        })
        .collect();
    assert!(texts.iter().any(|r| r.text.contains("Hello") && !r.style.bold));
    assert!(texts.iter().any(|r| r.text.contains("world") && r.style.bold));
}
```

`StyledSpan` is a new struct in `flow.rs`:

```rust
/// A run of text with a single style, used inside RichParagraph.
#[derive(Debug, Clone)]
pub struct StyledSpan {
    pub text: String,
    pub style: TextStyle,
}
```

(Adjust the assertion to however `DrawCommand::Text`/`TextRun` actually exposes text and style — read `crates/perfect-print-core/src/draw.rs` first and keep the test's *intent*: two runs, correct styles, second run offset right of the first.)

- [ ] **Step 2: Run** `cargo test -p perfect-print-layout test_rich_paragraph` — expect compile FAIL (no `RichParagraph` variant).

- [ ] **Step 3: Implement.** Add the variant to `ContentBlock`:

```rust
/// A paragraph with mixed inline styles.
RichParagraph {
    spans: Vec<StyledSpan>,
    /// Alignment, line-height, and paragraph-level defaults come from here.
    base_style: TextStyle,
},
```

Implementation approach in the layout engine: the simplest **correct** implementation shapes each span with its own style, concatenates the shaped words into one line-breaking stream (line breaking across span boundaries must work — "Hello **world** again" can wrap between any words), and emits one `DrawCommand::Text` per (line × span-fragment). Reuse `ParagraphEngine`'s existing shaping/word-measurement path; do not duplicate shaping logic — refactor `ParagraphEngine` so its word-measuring step takes `(text, style)` pairs. Height/pagination handling mirrors the existing `Paragraph` arm in `FlowLayoutEngine::layout()`.

- [ ] **Step 4: Run** `cargo test -p perfect-print-layout` — all tests pass, including the new one.

- [ ] **Step 5: Public API.** In `crates/perfect-print/src/lib.rs` add:

```rust
/// A paragraph mixing plain, bold, italic, and styled spans.
pub struct RichParagraph {
    spans: Vec<TextSpan>,
    base: TextStyle,
}

impl RichParagraph {
    pub fn new() -> Self { /* base = document-default-compatible TextStyle */ }
    pub fn span(mut self, span: TextSpan) -> Self { /* push */ }
    pub fn text(mut self, s: impl Into<String>) -> Self { /* plain span */ }
    pub fn bold(mut self, s: impl Into<String>) -> Self { /* bold span */ }
    pub fn italic(mut self, s: impl Into<String>) -> Self { /* italic span */ }
    pub fn align(mut self, a: TextAlign) -> Self { /* set on base */ }
    pub fn font_size(mut self, size: f64) -> Self { /* set on base AND all spans lacking explicit size — check how TextSpan stores style */ }
}

impl From<RichParagraph> for ContentBlock { /* map to RichParagraph variant */ }
```

`TextSpan` already exists in `lib.rs` (line ~843) — reuse it; convert to `StyledSpan` in the `From` impl.

- [ ] **Step 6: Add a public-API test** in `crates/perfect-print/src/lib.rs` (or `tests/`): build a document with a `RichParagraph`, `build()`, assert page_count == 1 and `text_content()` contains both span texts. Run `cargo test -p perfect-print`.

- [ ] **Step 7: Commit** — `git commit -m "feat(layout): rich paragraphs with mixed inline styles"`.

---

### Task 2: List blocks (core API improvement)

**Files:**
- Modify: `crates/perfect-print-layout/src/flow.rs`
- Modify: `crates/perfect-print/src/lib.rs`

- [ ] **Step 1: Failing test** in `flow.rs`:

```rust
#[test]
fn test_bulleted_list_indents_and_prefixes() {
    let style = TextStyle::new(FontRef::new("Helvetica"), 12.0);
    let block = ContentBlock::List {
        items: vec![
            ListItem { spans: vec![StyledSpan { text: "First".into(), style: style.clone() }], level: 0 },
            ListItem { spans: vec![StyledSpan { text: "Second".into(), style: style.clone() }], level: 0 },
        ],
        kind: ListKind::Bulleted,
        style: style.clone(),
    };
    let mut engine = FlowLayoutEngine::new(FlowConfig::default());
    let model = engine.layout(&[block]);
    let text = /* collect all text-run text */;
    assert!(text.contains("First") && text.contains("Second"));
    // Bullet markers present:
    assert!(text.contains('\u{2022}'));
}
```

New types:

```rust
#[derive(Debug, Clone)]
pub enum ListKind { Bulleted, Numbered }

#[derive(Debug, Clone)]
pub struct ListItem {
    pub spans: Vec<StyledSpan>,
    /// Nesting depth, 0-based. Each level indents 18pt further.
    pub level: usize,
}
```

- [ ] **Step 2: Run to fail**, then **implement**: lower each `ListItem` into a `RichParagraph` whose spans are `[marker_span] + item.spans`, laid out with a left inset of `18.0 * (level + 1)` points (implement inset by reducing the max width and offsetting x — follow how `blockquote`-style indent is simplest in the current engine; if there is no indent mechanism, add `indent_left: f64` to the RichParagraph variant and honor it in layout, defaulting 0). Numbered lists emit `"1. "`, `"2. "`, … per level-0 sequence (nested numbering restarts).

- [ ] **Step 3: Run** `cargo test -p perfect-print-layout` — pass.

- [ ] **Step 4: Public API**: `pub struct List` with `List::bulleted() / List::numbered()`, `.item(impl Into<String>)`, `.rich_item(RichParagraph)`, `.nested(List)` (flattens with level+1); `From<List> for ContentBlock`. Test: document with a 3-item list builds and `text_content()` shows all items.

- [ ] **Step 5: Commit** — `git commit -m "feat(layout): bulleted and numbered list blocks"`.

---

### Task 3: Verify + finish style inheritance (deferred item 6 from IMPROVEMENT-PLAN)

`FlowConfig.default_style` and `Document::default_style()` both exist — but IMPROVEMENT-PLAN.md marks item 6 "Deferred". Determine whether the wiring is real.

**Files:**
- Read: `crates/perfect-print/src/lib.rs:95`, `crates/perfect-print-layout/src/flow.rs`
- Possibly modify: `flow.rs` layout paragraph arm
- Test: `flow.rs`

- [ ] **Step 1: Write the test first** (it documents the contract either way):

```rust
#[test]
fn test_paragraph_inherits_flow_default_style() {
    let mut config = FlowConfig::default();
    let mut default = TextStyle::new(FontRef::new("Times New Roman"), 14.0);
    default.color = Color::rgb(1.0, 0.0, 0.0);
    config.default_style = Some(default);
    // A paragraph built with a marker "unset" style — check how the public
    // Document builder passes paragraphs without explicit styles; the
    // inheritance semantics implemented must match that path.
    ...
}
```

First read the code: if inheritance is already correctly wired from `Document::default_style()` through to rendered runs, write the test proving it, flip item 6 to Done in `docs/IMPROVEMENT-PLAN.md`, commit. If not wired (or only partially), implement merge semantics: paragraph explicitly-set fields win, unset fields fall back to the document default. Since `TextStyle` has no `Option` fields for font/size, the pragmatic merge is: the public `Paragraph` builder tracks *which* fields were explicitly set (it already has `Option`-shaped builder state — check `Paragraph` struct at `lib.rs:585`) and applies defaults at `build()` time.

- [ ] **Step 2: Commit** — `git commit -m "feat(api): document default style inheritance (or test+doc if already wired)"`.

---

### Task 4: CSS subset parser (`perfect-print-html`)

Pure functions, no DOM dependency — highly unit-testable.

**Files:**
- Create: `crates/perfect-print-html/src/css.rs` (tokenizer for declarations, values, colors, lengths)
- Create: `crates/perfect-print-html/src/stylesheet.rs` (rules, selectors, specificity, cascade)
- Modify: `crates/perfect-print-html/src/lib.rs` (add `mod css; mod stylesheet;`)
- Modify: `crates/perfect-print-html/Cargo.toml` (add `perfect-print-core` workspace dep)

- [ ] **Step 1: Failing tests first** in `css.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use perfect_print_core::color::Color;

    #[test]
    fn parses_declarations() {
        let d = parse_declarations("font-size: 14pt; color: #ff0000; font-weight: bold");
        assert_eq!(d.len(), 3);
        assert_eq!(d[0], Declaration { property: "font-size".into(), value: "14pt".into() });
    }

    #[test]
    fn parses_lengths() {
        assert_eq!(parse_length("14pt", 12.0), Some(14.0));
        assert_eq!(parse_length("16px", 12.0), Some(12.0)); // px × 0.75
        assert_eq!(parse_length("1.5em", 12.0), Some(18.0)); // em × parent size
        assert_eq!(parse_length("12", 12.0), Some(12.0));    // bare number = pt
        assert_eq!(parse_length("banana", 12.0), None);
    }

    #[test]
    fn parses_colors() {
        assert_eq!(parse_color("#ff0000"), Some(Color::rgb(1.0, 0.0, 0.0)));
        assert_eq!(parse_color("#f00"), Some(Color::rgb(1.0, 0.0, 0.0)));
        assert_eq!(parse_color("rgb(0, 128, 255)"), Some(Color::rgb(0.0, 128.0/255.0, 1.0)));
        assert_eq!(parse_color("red"), Some(Color::rgb(1.0, 0.0, 0.0)));
        assert_eq!(parse_color("chartreuse-ish"), None);
    }
}
```

Check `Color`'s actual constructor/representation in `perfect-print-core/src/color.rs` first (0–1 floats vs 0–255 u8) and match it.

- [ ] **Step 2: Implement `css.rs`:**

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Declaration { pub property: String, pub value: String }

/// Split "a: b; c: d" into declarations. Lowercases property names,
/// trims values, skips malformed entries (collecting no error — the
/// caller records warnings).
pub fn parse_declarations(input: &str) -> Vec<Declaration> { ... }

/// Lengths → points. `parent_size` resolves em. Returns None for unknown units.
pub fn parse_length(value: &str, parent_size: f64) -> Option<f64> { ... }

pub fn parse_color(value: &str) -> Option<Color> { ... } // 16 CSS basic colors + hex + rgb()
```

- [ ] **Step 3:** `cargo test -p perfect-print-html` — pass.

- [ ] **Step 4: Failing tests for `stylesheet.rs`:**

```rust
#[test]
fn cascade_specificity_id_beats_class_beats_tag() {
    let sheet = Stylesheet::parse("p { color: #00ff00 } .warn { color: #ffff00 } #boss { color: #ff0000 }");
    let d = sheet.matching_declarations("p", &["warn".into()], Some("boss"));
    // Highest-specificity color wins:
    assert_eq!(resolve_color(&d), Some(Color::rgb(1.0, 0.0, 0.0)));
}

#[test]
fn later_rule_wins_ties() { ... }

#[test]
fn at_page_rule_extracted() {
    let sheet = Stylesheet::parse("@page { size: a4; margin: 36pt }");
    assert_eq!(sheet.page_rule.as_ref().unwrap().size, Some(PageSizeSpec::A4));
}
```

- [ ] **Step 5: Implement `stylesheet.rs`:** rule = `Vec<SimpleSelector>` (comma list) + `Vec<Declaration>`; `SimpleSelector { tag: Option<String>, class: Option<String>, id: Option<String> }`; specificity fn; `Stylesheet::parse` strips `/* comments */`, extracts `@page` into `page_rule: Option<PageRule>`, ignores other at-rules with a warning list (`Stylesheet.warnings: Vec<String>`); `matching_declarations(tag, classes, id)` returns declarations sorted by (specificity, source order) so callers apply in order and last-write-wins.

- [ ] **Step 6:** `cargo test -p perfect-print-html` — pass. **Commit** — `git commit -m "feat(html): CSS subset parser with cascade and @page"`.

---

### Task 5: DOM → ContentBlock converter

The heart of the feature.

**Files:**
- Create: `crates/perfect-print-html/src/convert.rs`
- Modify: `crates/perfect-print-html/Cargo.toml` — add `scraper = "0.20"`, `perfect-print-layout`, `perfect-print` workspace-path deps
- Modify: `crates/perfect-print-html/src/lib.rs`
- Modify: `crates/perfect-print-html/src/error.rs` — rename `HtmlRenderStage::CreateWebView` → `Parse` (pure-Rust pipeline; fix all uses)

- [ ] **Step 1: Define the output type and top-level function signature** in `convert.rs`:

```rust
pub struct ConvertedDocument {
    pub blocks: Vec<ContentBlock>,
    pub title: Option<String>,
    pub page: PageSetup,          // size + margins resolved from @page or defaults
    pub images: Vec<LoadedImage>, // id + decoded ImageData, policy-checked
    pub warnings: Vec<String>,    // unsupported tags/properties encountered
}

pub fn convert(doc: &HtmlDocument) -> Result<ConvertedDocument, HtmlRenderError>
```

- [ ] **Step 2: Failing integration-style tests** (these define the whole behavior — write them all up front in `convert.rs` tests module):

```rust
fn blocks_of(html: &str) -> ConvertedDocument {
    convert(&HtmlDocument::new(html)).unwrap()
}

#[test]
fn paragraph_with_inline_bold() {
    let c = blocks_of("<p>Hello <b>world</b></p>");
    assert_eq!(c.blocks.len(), 1);
    let ContentBlock::RichParagraph { spans, .. } = &c.blocks[0] else { panic!() };
    assert_eq!(spans.len(), 2);
    assert!(!spans[0].style.bold && spans[1].style.bold);
    assert_eq!(spans[1].text, "world");
}

#[test]
fn headings_use_ua_defaults() {
    let c = blocks_of("<h1>Title</h1>");
    // 24pt bold per UA stylesheet
}

#[test]
fn style_block_and_inline_style_cascade() {
    let c = blocks_of(r#"<style>p { color: #00f } .big { font-size: 20pt }</style>
                        <p class="big" style="color: #f00">x</p>"#);
    // color red (inline wins), size 20 (class rule)
}

#[test]
fn nested_inline_styles_compose() {
    // <i><b>x</b></i> → span has bold && italic
}

#[test]
fn lists_convert() {
    let c = blocks_of("<ul><li>a</li><li>b</li></ul>");
    // → ContentBlock::List, Bulleted, 2 items
}

#[test]
fn ordered_list_is_numbered() { ... }

#[test]
fn tables_convert() {
    let c = blocks_of("<table><tr><th>H</th></tr><tr><td>c</td></tr></table>");
    // → ContentBlock::Table with 1 header column and 1 row; th bold
}

#[test]
fn hr_becomes_rule_and_br_breaks() { ... } // hr → thin full-width Commands rect; br splits spans with forced line break — if ParagraphEngine lacks forced breaks, emit separate RichParagraphs with zero inter-gap and record a warning-free result

#[test]
fn page_break_css() {
    let c = blocks_of(r#"<p>a</p><p style="page-break-before: always">b</p>"#);
    assert!(matches!(c.blocks[1], ContentBlock::PageBreak));
}

#[test]
fn at_page_sets_size() {
    let c = blocks_of("<style>@page { size: a4 }</style><p>x</p>");
    // c.page reflects A4 (595 × 842 pt)
}

#[test]
fn unknown_tag_warns_not_errors() {
    let c = blocks_of("<article><p>x</p></article><video src=\"x\"/>");
    assert!(!c.blocks.is_empty());
    assert!(c.warnings.iter().any(|w| w.contains("video")));
}

#[test]
fn img_data_uri_loads() {
    // 1×1 PNG data URI → images.len()==1, block is ContentBlock::Image
}

#[test]
fn img_file_outside_base_dir_rejected() {
    // policy without local base → warning + placeholder skip (offline default)
}

#[test]
fn title_extracted() {
    let c = blocks_of("<html><head><title>T</title></head><body><p>x</p></body></html>");
    assert_eq!(c.title.as_deref(), Some("T"));
}

#[test]
fn margins_between_blocks() {
    let c = blocks_of("<p>a</p><p>b</p>");
    // Gap block between the two paragraphs (UA margin collapse: max of bottom/top)
}
```

- [ ] **Step 3: Implement `convert.rs`.** Structure:

```rust
struct Converter<'a> {
    sheet: Stylesheet,        // UA sheet + document <style> sheets merged in order
    policy: &'a ResourcePolicy,
    blocks: Vec<ContentBlock>,
    images: Vec<LoadedImage>,
    warnings: Vec<String>,
    counter: usize,           // for generated image ids
}

/// Resolved style carried down the recursion (the cascade).
#[derive(Clone)]
struct ComputedStyle { /* mirrors TextStyle + margins + page-break flags */ }
```

Walk the `scraper::Html` tree recursively. For each element: match stylesheet rules (tag name, `class` attr split on whitespace, `id` attr), apply declarations in cascade order onto a `ComputedStyle` inherited from the parent (inherited props: font-*, color, text-align, line-height, letter-spacing; non-inherited: margins, decorations reset per CSS spec — decorations DO propagate visually, so keep underline/strike once set, matching real browser behavior). Then dispatch on tag:
- block containers (`div`, `body`, `blockquote`, unknown-block): recurse; `blockquote` adds indent via the Task 2 indent mechanism.
- paragraph-ish (`p`, `h1`–`h6`, `li` content): collect inline runs (text nodes + inline elements, whitespace-collapsed per HTML rules: runs of whitespace → single space, leading/trailing trimmed at block edges) into `RichParagraph`; emit surrounding `Gap`s from computed margins with adjacent-margin collapsing (track `pending_gap: f64`, emit `max(pending, margin_top)`).
- `ul`/`ol`: build `ContentBlock::List`, recursing for nesting.
- `table`: build `layout::table::{Column, Row, Cell}`; `th` → bold cell / header row; honor `border`, cell `padding`, `background-color`.
- `img`: resolve `src` through `ResourcePolicy::allows_url`/`allows_local_path`; decode with the `image` crate (workspace dep) for dimensions; width/height attrs or CSS override; violations → warning + skip.
- `hr` → `ContentBlock::Commands` with a 0.5pt full-content-width rect; `br` → forced break as noted in the test.
- text nodes outside any block → wrap in an implicit paragraph.

Whitespace collapsing is where naive implementations fail — implement it as a distinct pure function with its own unit tests:

```rust
/// Collapse HTML whitespace: any run of [\t\n\r ] → single space.
fn collapse_whitespace(s: &str) -> String { ... }
```

- [ ] **Step 4:** `cargo test -p perfect-print-html` — all convert tests pass.

- [ ] **Step 5: Commit** — `git commit -m "feat(html): DOM to ContentBlock converter with CSS cascade"`.

---

### Task 6: Public rendering API + integration with `perfect-print`

**Files:**
- Modify: `crates/perfect-print-html/src/lib.rs`
- Create: `crates/perfect-print-html/tests/render.rs`

- [ ] **Step 1: Failing integration test** `tests/render.rs`:

```rust
use perfect_print_html::HtmlDocument;

#[test]
fn html_renders_to_pdf_bytes() {
    let doc = HtmlDocument::new("<h1>Report</h1><p>Hello <b>world</b></p>");
    let result = doc.render().unwrap();
    assert!(result.model.page_count() >= 1);
    let pdf = result.to_pdf_bytes().unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    assert!(result.warnings.is_empty());
}

#[test]
fn deterministic_output() {
    let html = "<h1>Same</h1><p>Every time</p>";
    let a = HtmlDocument::new(html).render().unwrap().to_pdf_bytes().unwrap();
    let b = HtmlDocument::new(html).render().unwrap().to_pdf_bytes().unwrap();
    assert_eq!(a, b);
}

#[test]
fn multi_page_html_paginates() {
    let body: String = (0..200).map(|i| format!("<p>Paragraph {i}</p>")).collect();
    let result = HtmlDocument::new(body).render().unwrap();
    assert!(result.model.page_count() > 1);
}
```

- [ ] **Step 2: Implement** on `HtmlDocument`:

```rust
pub struct HtmlRenderResult {
    pub model: DocumentModel,
    pub warnings: Vec<String>,
}

impl HtmlRenderResult {
    pub fn to_pdf_bytes(&self) -> Result<Vec<u8>, HtmlRenderError> { ... } // via perfect-print-pdf; enforce policy.validate_pdf_bytes
    pub fn save_pdf(&self, path: impl AsRef<Path>) -> Result<(), HtmlRenderError> { ... }
    pub fn render_png(&self, dir: impl AsRef<Path>, dpi: u32) -> Result<Vec<PathBuf>, HtmlRenderError> { ... }
}

impl HtmlDocument {
    /// Validate → parse → cascade → convert → flow layout → DocumentModel.
    pub fn render(&self) -> Result<HtmlRenderResult, HtmlRenderError> { ... }
    /// Convenience: straight to PDF file.
    pub fn save_pdf(&self, path: impl AsRef<Path>) -> Result<(), HtmlRenderError> { ... }
}
```

`render()` runs `self.validate()` first; converts each pipeline error into `HtmlRenderError::at_stage` with the right stage (`Parse`, `Fonts`, `Images`, `RenderPdf`, `ValidatePdf`). Page setup: explicit `page_settings()` set by the caller wins over `@page`; `@page` wins over letter default. Wire the title: converted `<title>` used unless `.title()` was set. `ReadinessTracker` (`readiness.rs`) — read it and mark whatever stages it tracks as the pipeline advances; if it doesn't fit the pure-Rust pipeline, simplify it in the same spirit rather than leaving dead code.

- [ ] **Step 3:** `cargo test -p perfect-print-html` — pass. Also run `cargo test --workspace` (renamed enum variant may break other crates — fix all references).

- [ ] **Step 4: Commit** — `git commit -m "feat(html): render() pipeline — HTML to DocumentModel/PDF/PNG"`.

---

### Task 7: CLI `render-html` command

**Files:**
- Modify: `crates/perfect-print-cli/src/main.rs` (find the clap subcommand enum; follow existing `render` subcommand pattern exactly)
- Modify: `crates/perfect-print-cli/Cargo.toml` (dep on `perfect-print-html`)

- [ ] **Step 1:** Add subcommand:

```
render-html <input.html> [--pdf out.pdf] [--png-dir dir/ --dpi 300] [--base-dir DIR] [--strict]
```

`--base-dir` → `ResourcePolicy::with_local_base_directory`; `--strict` → any conversion warning becomes exit code 1 with warnings printed to stderr; default prints warnings to stderr but succeeds.

- [ ] **Step 2: Verify end-to-end by hand:**

```bash
cat > /tmp/pp-demo.html <<'EOF'
<style>
  @page { size: letter; margin: 54pt }
  h1 { color: #003366 }
  .highlight { background-color: #ffffcc }
</style>
<h1>Perfect Print HTML Demo</h1>
<p>This document was written in <b>HTML</b> and rendered by the
<i>pure-Rust</i> perfect-print pipeline.</p>
<ul><li>Deterministic bytes</li><li>No browser</li><li>CI-friendly</li></ul>
<table border="1"><tr><th>Feature</th><th>Status</th></tr>
<tr><td>CSS cascade</td><td>Yes</td></tr></table>
EOF
cargo run -p perfect-print-cli -- render-html /tmp/pp-demo.html --pdf /tmp/pp-demo.pdf --png-dir /tmp/pp-demo-pages --dpi 150
```

Expected: exit 0, PDF exists and starts with `%PDF`, at least one PNG page produced. Open/inspect the PNG (e.g. `Read` the PNG file) and confirm: heading is dark blue and large, bold/italic render, bullets show, table has borders.

- [ ] **Step 3: Commit** — `git commit -m "feat(cli): render-html subcommand"`.

---

### Task 8: Docs, README, fuzz target, wrap-up

**Files:**
- Modify: `README.md` — add HTML/CSS to Features, Architecture, Crate Structure, Verification Commands, plus a short "HTML to PDF" example (use the exact `HtmlDocument::new(...).save_pdf(...)` API from Task 6)
- Modify: `docs/IMPROVEMENT-PLAN.md` — mark item 6 done (per Task 3 outcome); add a dated section listing: rich spans, lists, HTML/CSS pipeline shipped
- Create: `docs/html-css-support.md` — copy the "Supported HTML/CSS subset" contract section from this plan, plus the graceful-degradation policy and `ResourcePolicy` security model (offline default, base-dir sandboxing)
- Create: `fuzz/fuzz_targets/fuzz_html_convert.rs` — mirror `fuzz_document_model_json.rs` structure: `fuzz_target!(|data: &[u8]| { if let Ok(s) = std::str::from_utf8(data) { let _ = perfect_print_html::HtmlDocument::new(s).render(); } });` and register it in `fuzz/Cargo.toml`. Build-check with `cargo check` on the fuzz package only (don't run the fuzzer).

- [ ] **Step 1:** Make all doc edits.
- [ ] **Step 2:** Final verification: `cargo test --workspace` all green; `cargo build --workspace` no new warnings; re-run the Task 7 end-to-end command.
- [ ] **Step 3: Commit** — `git commit -m "docs: HTML/CSS support documentation and fuzz target"`.

---

## Self-review notes (already applied)

- The plan intentionally instructs the executor to *read the real definitions* (`draw.rs` TextRun shape, `Paragraph` builder internals, CLI subcommand pattern) before writing code that touches them — line numbers drift.
- All HTML behavior is pinned by the Task 5 test list; anything not covered there falls under the graceful-degradation warning policy.
- Type names used across tasks: `StyledSpan`, `ListItem`, `ListKind`, `ContentBlock::RichParagraph`, `ConvertedDocument`, `HtmlRenderResult` — consistent throughout.
- Escape hatch: if `scraper 0.20` has API drift, any recent `scraper` version is fine; only `Html::parse_document`, element tag/attr access, and text traversal are needed.
