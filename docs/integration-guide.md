# Integration Guide

## Quick Start

Add `perfect-print` to your `Cargo.toml`:

```toml
[dependencies]
perfect-print = "0.1"
```

Create a document, render to PDF, and print:

```rust
use perfect_print::Document;
use perfect_print::page::PageSize;

let model = Document::new()
    .title("My Document")
    .page(PageSize::Letter)
    .paragraph("Hello, World!")
    .build()?;

// Render to PDF
let pdf_bytes = model.render_pdf()?;

// Print (macOS)
#[cfg(target_os = "macos")]
{
    let dialog = perfect_print_backend_macos::MacosPrintDialog::new();
    let job_id = dialog.submit_print_job(&pdf_path, &PrintSettings::default())?;
}
```

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Your Application                      │
├─────────────────────────────────────────────────────────┤
│  perfect-print (ergonomic API)                          │
│    Document, Paragraph, Table, Image, PageBreak          │
├─────────────────────────────────────────────────────────┤
│  perfect-print-layout (text shaping, flow, pagination)  │
│    rustybuzz, fontdb, unicode-bidi, hyphenation         │
├─────────────────────────────────────────────────────────┤
│  perfect-print-core (canonical page model)              │
│    Page, Layer, DrawCommand, Style, Color, Font, Image   │
├──────────────┬──────────────────┬───────────────────────┤
│  PDF output  │  Raster output   │  Native print         │
│  lopdf       │  tiny-skia       │  platform backend     │
│  (pure Rust) │  (pure Rust)     │  (lpstat/lp/CUPS/...) │
└──────────────┴──────────────────┴───────────────────────┘
```

## The Canonical Page Model

All output paths consume the same `DocumentModel`. This is the core WYSIWYG guarantee:

1. **Build** a `DocumentModel` using the ergonomic API
2. **Layout** is computed once (text shaping, pagination, table sizing)
3. **Render** to any output: PDF, raster, or native print

```rust
// One model, three outputs
let model = build_document()?;

// PDF
let pdf = perfect_print_pdf::PdfRenderer::new().render_to_pdf(&model, "out.pdf")?;

// Raster (PNG pages)
let renderer = perfect_print_render::TinySkiaRenderer::new();
let pngs = renderer.render_to_raster(&model, Dpi(300.0), "pages/")?;

// Native print
let dialog = perfect_print_backend_macos::MacosPrintDialog::new();
dialog.submit_print_job(&pdf_path, &PrintSettings::default())?;
```

## Document Builder

```rust
use perfect_print::Document;
use perfect_print::page::PageSize;
use perfect_print::units::Point;
use perfect_print::color::Color;

let model = Document::new()
    .title("Invoice #1234")
    .author("My App")
    .default_style(TextStyle::new("Helvetica", 10.0))
    .page(PageSize::Letter)
    .paragraph("INVOICE")
        .font("Helvetica")
        .size(18.0)
        .align(TextAlign::Center)
    .paragraph("Thank you for your business.")
        .size(10.0)
    .page_break()
    .paragraph("Second page content")
    .build()?;
```

## Text Styling

Styles cascade: document default → paragraph → text run.

```rust
// Document-wide default
Document::new()
    .default_style(TextStyle::new("Helvetica", 12.0))
    .page(PageSize::Letter)
    // Paragraph inherits document default, then overrides
    .paragraph("Title")
        .size(24.0)          // Override size
        .color(Color::blue()) // Override color
    .build()?;
```

## Tables

```rust
use perfect_print::Table;

let table = Table::new()
    .column_width(200.0)  // Fixed width
    .column_width(100.0)
    .column_width(100.0)
    .header_row()
        .cell("Item")
        .cell("Qty")
        .cell("Price")
    .row()
        .cell("Widget A")
        .cell("2")
        .cell("$9.99")
    .row()
        .cell("Gadget B")
        .cell("1")
        .cell("$24.99");

let model = Document::new()
    .page(PageSize::Letter)
    .table(table)
    .build()?;
```

## Images

```rust
use perfect_print::Image;

let image = Image::from_file("photo.jpg")?
    .dest_rect(Rect::new(72.0, 72.0, 200.0, 200.0));

let model = Document::new()
    .page(PageSize::Letter)
    .image(image)
    .build()?;
```

## Page Size Presets

```rust
PageSize::Letter      // 8.5 x 11 in (612 x 792 pt)
PageSize::A4          // 210 x 297 mm (595 x 842 pt)
PageSize::Legal       // 8.5 x 14 in
PageSize::Tabloid     // 11 x 17 in
PageSize::A3          // 297 x 420 mm
PageSize::A5          // 148 x 210 mm
PageSize::Custom { width: 216.0, height: 720.0 } // Custom
```

## Strictness Modes

Control how the engine handles unsupported settings:

```rust
use perfect_print_core::Strictness;

// BestEffort: silently apply fallbacks
model.print_with(Strictness::BestEffort)?;

// Warn: print warnings but continue (default)
model.print_with(Strictness::Warn)?;

// Exact: fail on any unsupported setting
model.print_with(Strictness::Exact)?;
```

## Error Handling

All errors are structured via `PrintError`:

```rust
match model.validate() {
    Ok(warnings) => {
        for w in &warnings {
            eprintln!("Warning: {}", w);
        }
    }
    Err(PrintError::UnsupportedPaperSize { requested, fallback }) => {
        eprintln!("Paper {:?} not supported, using {:?}", requested, fallback);
    }
    Err(PrintError::NoPrinters) => {
        eprintln!("No printers available");
    }
    Err(e) => eprintln!("Error: {}", e),
}
```

## Diagnostics

Generate a full diagnostics bundle for debugging:

```bash
# Build the CLI
cargo build -p perfect-print-cli

# Generate diagnostics zip
cargo run -p perfect-print-cli -- diagnostics hello --out hello-diag.zip

# The zip contains:
#   output.pdf       — rendered PDF
#   pages/*.png      — raster page renders
#   model.json       — serialized document model
#   system-info.json — OS, architecture, timestamp
#   font-list.json   — all system font families
#   printers.json    — printer capabilities
```

## Platform Notes

### macOS
- Interactive printing uses a standard `NSPrintPanel` (`NSPrintOperation` + PDFKit)
  with Copies, Pages, Paper Size, Orientation, Scale, Preview, Page Setup, and the PDF menu
- There is no in-app sheet. Scale lives on the native panel only
- `PrintSettings` paper / orientation / scaling / duplex are initial defaults; the user can change them in the sheet
- Canonical models render once to in-memory PDF bytes before the panel opens
- Unattended submission, job tracking, and cancellation remain available through
  the `lpstat`/`lp`/`cancel` bridge

#### Single full-chart page (Families and other posters)

Send **one PDF page** that contains the entire chart image. Do not pre-split
the chart into N Letter tiles at 100% (1px = 1pt) before the panel opens.

`PerfectPrintPDFView` then owns pagination:

1. **Fit / scale-to-fit** the imageable area (paper minus the 28pt join,
   intersected with `imageablePageBounds`) → **1 page**
2. **Scale up** on the native panel → **N** row-major poster tiles
   (left→right, then top→bottom). Each page is a contiguous crop
3. **Scale down** → fewer tiles; **1 page** when the chart fits

Tile look: no content overlap, 28pt join, ≤8pt registration ticks in the
margin, “n of N” in the margin only (hidden when N = 1). The label is the
post-scale page count — do not draw it in the PDF if you are sending a
single full-chart page (the print view adds it). Do not add a Families
poster-scale slider; Scale stays on `NSPrintPanel`.

```rust
// Families / chart print: one page, full raster, FitToPage default.
// The user raises or lowers Scale in the system panel; page count follows.
let model = Document::new()
    .title("Pedigree")
    .page(page_size)          // Letter or landscape Letter
    .margin(28.0)             // join strip; do not pre-tile
    .add_image("chart", width, height, pixels)
    .add(Image::new("chart").size(width as f64, height as f64)) // 1px = 1pt
    .build()?;

let settings = PrintSettings::default()
    .paper_size(page_size)
    .scaling(PrintScaling::FitToPage);

perfect_print_backend_macos::print_pdf_bytes_with_dialog(
    &model.render_pdf()?,
    Some("Pedigree"),
    &settings,
)?;
```

Sit-with: with a 1-page full-chart PDF, Scale down → 1 page in Preview;
Scale up → multi-page tiles.

### Windows
- Backend is a stub — PDF/raster output works
- Native printing not yet implemented

### Linux
- Backend is a stub — PDF/raster output works
- Native printing not yet implemented

## GUI Integration

For GUI apps, use the `PrintDialog` trait:

```rust
trait PrintDialog {
    fn show_print_dialog(&self, settings: &PrintSettings, title: Option<&str>)
        -> Result<PrintSettings, PrintError>;
    fn show_page_setup(&self, settings: &PrintSettings)
        -> Result<PrintSettings, PrintError>;
    fn available_printers(&self) -> Result<Vec<Printer>, PrintError>;
    fn default_printer(&self) -> Result<Printer, PrintError>;
}
```

Implement this trait for your GUI framework, or use the provided backends directly.
