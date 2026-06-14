# perfect-print

**A Rust print/document rendering API with PDF, PNG, CLI, and print-backend support.**

A Rust developer can create a document in under 20 lines, export it to PDF,
render PNG previews, and submit print jobs through the available backend. The
project uses one canonical page model so output paths can be measured and tested
against each other. Native GUI print dialogs are still backend-specific work in
progress.

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

## Features

- **One canonical page model** — PDF, raster, preview, and print all consume the same model
- **Exact units** — points, inches, mm, px-at-DPI
- **Text shaping** — rustybuzz-powered shaping with bidi, ligatures, and kerning foundations
- **Image support** — PNG/JPEG loading, rendering in both raster and PDF backends
- **PDF output** — with embedded images (FlateDecode XObjects) and text output
- **Raster output** — via tiny-skia, any DPI
- **Print backend** — macOS via CUPS (`lp`/`lpstat`); other backends are still maturing
- **Visual diff CLI** — pixel-by-pixel PNG comparison with heatmaps
- **Geometry assertions** — structured checks for page size, content bounds, text baselines
- **Deterministic output** — identical documents produce byte-identical bytes
- **Strictness modes** — BestEffort, Warn (default), Exact
- **CI-friendly** — no physical printer required for tests

## Architecture

```
perfect-print/          Ergonomic public API (Document, Paragraph, Image)
perfect-print-core/     Canonical document model, units, pages, draw commands
perfect-print-layout/   Text shaping, flow layout, pagination, tables
perfect-print-render/   Raster renderer (tiny-skia)
perfect-print-pdf/      PDF generator (lopdf)
perfect-print-dialog/   Print settings, printer capabilities
perfect-print-backend-macos/   macOS print backend (CUPS)
perfect-print-cli/      CLI for render, verify, print, diagnostics, printers
```

## Verification Commands

```bash
# Build everything
cargo build --workspace

# Run all tests
cargo test --workspace

# Render an example to PDF + PNG
cargo run -p perfect-print-cli -- render hello --pdf output.pdf --png-dir pages/

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
| `perfect-print-render` | Raster rendering (tiny-skia) |
| `perfect-print-pdf` | PDF generation (lopdf) |
| `perfect-print-dialog` | Print settings, capabilities |
| `perfect-print-backend-macos` | macOS CUPS backend |
| `perfect-print-cli` | CLI + golden tests |

## License

MIT OR Apache-2.0
