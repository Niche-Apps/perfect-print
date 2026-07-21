# perfect-print

**A Rust print/document rendering API with PDF, PNG, CLI, and print-backend support.**

A Rust developer can create a document in under 20 lines, export it to PDF,
render PNG previews, and submit print jobs through the available backend. The
project uses one canonical page model so output paths can be measured and tested
against each other. On macOS, canonical documents and existing PDF bytes open a
real `NSPrintPanel` backed by `NSPrintOperation`; the CLI job API remains available
for unattended submission and queue management.

## Quick Start

```rust
use perfect_print::{Document, Paragraph, Color};

// Create a document in 5 lines
let doc = Document::new()
    .title("My First Document")
    .add(Paragraph::new("Hello, World!").font_size(24.0).bold())
    .add(Paragraph::new("This is a simple document."))
    .build();

// Export to PDF
doc.save_pdf("output.pdf")?;

// Render to PNG (for preview)
let paths = doc.render_png("output-pages", 300)?;
```

## HTML to PDF

`perfect-print-html` renders a supported HTML/CSS subset straight to the
canonical document model — no browser or WebView involved, so output is
deterministic and CI-testable. See
[`docs/html-css-support.md`](docs/html-css-support.md) for the full supported
tag/CSS-property list and the graceful-degradation policy.

```rust
use perfect_print_html::HtmlDocument;

let doc = HtmlDocument::new(
    "<h1>Report</h1><p>Hello <b>world</b>, rendered by <i>pure Rust</i>.</p>",
);
doc.save_pdf("report.pdf")?;

// Or drive the pipeline manually to also get PNG pages and warnings:
let result = doc.render()?;
result.save_pdf("report.pdf")?;
let pages = result.render_png("report-pages", 300)?;
for warning in &result.warnings {
    eprintln!("warning: {warning}");
}
```

## Features

- **One canonical page model** — PDF, raster, preview, and print all consume the same model
- **Exact units** — points, inches, mm, px-at-DPI
- **Text shaping** — rustybuzz-powered shaping with bidi, ligatures, and kerning foundations
- **Rich text** — mixed-style paragraphs (`RichParagraph`) and bulleted/numbered lists (`List`), inheriting document-level default styles
- **HTML/CSS rendering** — pure-Rust HTML/CSS subset → `DocumentModel` → PDF/PNG/print, no browser or WebView (see `perfect-print-html`)
- **Physical CSS length units** — `in`, `cm`, `mm`, `pc` (alongside `pt`/`px`/`em`) resolve to points anywhere a CSS length is accepted, including `@page { size: 8.5in 11in }`
- **`position: absolute`** — absolutely positioned elements (`left`/`top`/`width` in any supported unit) render at their authored coordinates via `ContentBlock::Positioned`, out of the normal document flow; see [`docs/html-css-support.md`](docs/html-css-support.md#position-absolute) for the supported subset and limitations
- **Image support** — PNG/JPEG loading, rendering in both raster and PDF backends
- **PDF output** — with embedded images (FlateDecode XObjects), embedded fonts (including the correct bold/italic face, not a synthetic regular face), and text output
- **Raster output** — via tiny-skia, any DPI
- **Print backend** — macOS via CUPS (`lp`/`lpstat`); other backends are still maturing
- **Visual diff CLI** — pixel-by-pixel PNG comparison with heatmaps
- **Geometry assertions** — structured checks for page size, content bounds, text baselines
- **Deterministic output** — identical documents produce byte-identical bytes
- **Strictness modes** — BestEffort, Warn (default), Exact
- **CI-friendly** — no physical printer required for tests

## Architecture

```
perfect-print/          Ergonomic public API (Document, Paragraph, RichParagraph, List, Image)
perfect-print-core/     Canonical document model, units, pages, draw commands
perfect-print-layout/   Text shaping, flow layout, pagination, tables
perfect-print-html/     HTML/CSS subset → ContentBlocks (scraper + hand-rolled CSS cascade)
perfect-print-render/   Raster renderer (tiny-skia)
perfect-print-pdf/      PDF generator (lopdf)
perfect-print-dialog/   Print settings, printer capabilities
perfect-print-backend-macos/   macOS print backend (CUPS)
perfect-print-cli/      CLI for render, render-html, verify, print, diagnostics, printers
```

The HTML pipeline is pure Rust end to end: `scraper`/`html5ever` parses the
DOM, a hand-rolled CSS subset parser resolves the cascade (inline `style=`,
`<style>` blocks, `@page`), and the styled DOM is lowered into the same
`ContentBlock`s (`RichParagraph`, `List`, `Table`, `Image`, ...) that
`perfect-print`'s own `Document` builder produces — so it reuses the existing
`FlowLayoutEngine` → `DocumentModel` → PDF/raster/print backends rather than
introducing a second rendering path.

## Verification Commands

```bash
# Build everything
cargo build --workspace

# Run all tests
cargo test --workspace

# Render an example to PDF + PNG
cargo run -p perfect-print-cli -- render hello --pdf output.pdf --png-dir pages/

# Render HTML/CSS to PDF + PNG (pure-Rust pipeline, no browser)
cargo run -p perfect-print-cli -- render-html input.html --pdf output.pdf --png-dir pages/ --dpi 300

# List printers
cargo run -p perfect-print-cli -- printers

# Show printer capabilities
cargo run -p perfect-print-cli -- capabilities --printer "My Printer"

# Verify visual parity
cargo run -p perfect-print-cli -- verify hello --against reference-images/ --tolerance 0.01

# Generate diagnostics bundle
cargo run -p perfect-print-cli -- diagnostics hello --output diag/

# Print to a printer (macOS)
cargo run -p perfect-print-cli -- print hello --printer "My Printer"
```

## Examples

### Invoice
```rust
use perfect_print::{Document, Paragraph, Gap, PageBreak};

let doc = Document::new()
    .title("Invoice #001")
    .add(Paragraph::new("INVOICE").font_size(18.0).bold())
    .add(Gap(12.0))
    .add(Paragraph::new("Customer: Acme Corp"))
    .add(Paragraph::new("Date: 2026-06-09"))
    .add(Gap(24.0))
    // ... table of items ...
    .save_pdf("invoice.pdf")?;
```

### Document with Image
```rust
use perfect_print::{Document, Paragraph, Image};

let doc = Document::new()
    .add(Paragraph::new("Product Catalog").font_size(24.0).bold())
    .add(Gap(12.0))
    .add(Image::new("product-photo").size(200.0, 150.0))
    .add(Paragraph::new("Product description here."))
    .build();
doc.save_pdf("catalog.pdf")?;
```

## Crate Structure

| Crate | Purpose |
|-------|---------|
| `perfect-print` | Public ergonomic API |
| `perfect-print-core` | Canonical model, units, draw commands |
| `perfect-print-layout` | Text shaping, flow, tables |
| `perfect-print-html` | HTML/CSS subset → `ContentBlock`s (pure Rust, no WebView) |
| `perfect-print-render` | Raster rendering (tiny-skia) |
| `perfect-print-pdf` | PDF generation (lopdf) |
| `perfect-print-dialog` | Print settings, capabilities |
| `perfect-print-backend-macos` | macOS CUPS backend |
| `perfect-print-cli` | CLI (`render`, `render-html`, ...) + golden tests |

## License

MIT OR Apache-2.0
