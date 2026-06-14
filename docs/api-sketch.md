# API Sketch

**Date:** 2026-06-08
**Status:** Draft

## Design Principles

1. **Builder-style API** for ergonomics
2. **Explicit over implicit** for advanced control
3. **One canonical page model** feeds all outputs
4. **No silent failures** - unsupported settings produce structured errors
5. **20-line hello world** as the complexity budget

## Hello World Example

```rust
use perfect_print::*;

fn main() -> Result<(), PerfectPrintError> {
    let doc = Document::new()
        .page_size(PageSize::Letter)
        .margin(72.0) // 1 inch
        .add(Page::new()
            .add(Text::new("Hello, perfect-print!")
                .font("Helvetica")
                .size(24.0)
                .bold())
            .add(Text::new("This is a WYSIWYG print library for Rust.")
                .font("Helvetica")
                .size(12.0)
                .position(72.0, 120.0)));

    // Preview in native window
    doc.preview()?;

    // Export to PDF
    doc.export_pdf("/tmp/hello.pdf")?;

    // Print with native dialog
    doc.print()?;

    Ok(())
}
```

## Invoice Example

```rust
use perfect_print::*;

fn main() -> Result<(), PerfectPrintError> {
    let doc = Document::new()
        .page_size(PageSize::Letter)
        .margin(54.0) // 0.75 inch
        .add(Page::new()
            .add(Header::new()
                .add(Text::new("INVOICE").size(28.0).bold())
                .add(Text::new("Invoice #001").size(10.0).right()))
            .add(Body::new()
                .add(Table::new()
                    .columns(&["Item", "Qty", "Price", "Total"])
                    .row(&["Widget A", "2", "$50.00", "$100.00"])
                    .row(&["Widget B", "1", "$75.00", "$75.00"])
                    .footer_row(&["", "", "Total:", "$175.00"]))
                .add(Paragraph::new()
                    .text("Thank you for your business!")
                    .align(Align::Center)))
            .add(Footer::new()
                .add(Text::new("Page {page} of {pages}").size(9.0).center())));

    doc.export_pdf("/tmp/invoice.pdf")?;
    doc.verify(Verbosity::Strict).assert_all()?;

    Ok(())
}
```

## Core Types

```rust
// Document root
pub struct Document { /* private */ }
impl Document {
    pub fn new() -> Self;
    pub fn page_size(self, size: PageSize) -> Self;
    pub fn margin(self, points: f64) -> Self;
    pub fn add_page(self, page: Page) -> Self;
    pub fn add_pages(self, pages: impl IntoIterator<Item = Page>) -> Self;
    pub fn resource_store(&self) -> &ResourceStore;

    // Output
    pub fn preview(&self) -> Result<Preview>;
    pub fn export_pdf(&self, path: &str) -> Result<>;
    pub fn export_png(&self, dir: &str, dpi: f64) -> Result<Vec<String>>;
    pub fn print(&self) -> Result<PrintJob>;
    pub fn print_with(&self, settings: PrintSettings) -> Result<PrintJob>;

    // Verification
    pub fn verify(&self, mode: VerifyMode) -> VerificationReport;
    pub fn to_model(&self) -> CanonicalModel;
}

impl DocumentBuilder for Document {
    fn build(&self) -> CanonicalModel { ... }
}

// Pages
pub struct Page { /* private */ }

// Elements
pub struct Text { /* private */ }
pub struct Paragraph { /* private */ }
pub struct Table { /* private */ }
pub struct Header { /* private */ }
pub struct Footer { /* private */ }
pub struct Image { /* private */ }
pub struct Rect { /* private */ }
pub struct Line { /* private */ }

pub enum TextAlign { Left, Center, Right, Justified }
pub enum PageSize {
    Letter, Legal, A4, A3, A5,
    // ... full ISO + US sizes
    Custom { width: Length, height: Length },
}
pub struct Length(f64, LengthUnit);
pub enum LengthUnit {
    Points,
    Inches,
    Mm,
    Px(f64), // px at given DPI
}

// Print settings
pub struct PrintSettings {
    pub printer: Option<String>,
    pub paper_size: Option<PageSize>,
    pub orientation: Option<Orientation>,
    pub duplex: Option<Duplex>,
    pub copies: u32,
    pub color_mode: Option<ColorMode>,
    pub quality: Option<PrintQuality>,
    pub strictness: Strictness,
}

pub enum Strictness {
    BestEffort,  // Try to print, report warnings
    Warn,        // Print only if non-destructive differences (default)
    Exact,       // Fail if any setting can't be honored
}
```

## Core Canonical Model

```rust
// perfect-print-core types

pub struct CanonicalModel {
    pub pages: Vec<PageModel>,
    pub resources: ResourceStore,
}

pub struct PageModel {
    pub size: Size<Points>,
    pub margins: Margins<Points>,
    pub layers: Vec<Layer>,
}

pub enum Layer {
    Foreground,
    Background,
    Header,
    Footer,
}

pub enum DrawCommand {
    TextRun(TextRunCmd),
    Rect(RectCmd),
    Path(PathCmd),
    Image(ImageCmd),
    Clip(ClipCmd),
    Transform(TransformCmd),
    Group(GroupCmd),
}

pub struct TextRunCmd {
    pub text: String,
    pub font_stack: Vec<FontRef>,
    pub size: f64, // points
    pub position: Point<Points>,
    pub style: TextStyle,
    pub color: Color,
    // Shaped glyphs computed during layout
    pub glyphs: Vec<ShapedGlyph>,
}

pub struct ShapedGlyph {
    pub glyph_id: u32,
    pub offset: Point<Points>,
    pub advance: Point<Points>,
    pub font_index: usize, // index into font_stack
    pub cluster: u32,      // index into original text string
}
```

## Verification API

```rust
pub struct VerificationReport {
    pub page_count: usize,
    pub page_sizes: Vec<Size<Points>>,
    pub content_bounds: Vec<Rect<Points>>,
    pub text_baselines: Vec<Vec<f64>>,
    pub table_rows: Vec<Vec<Rect<Points>>>,
    pub warnings: Vec<CapabilityWarning>,
    pub errors: Vec<PrintError>,
}

pub enum CapabilityWarning {
    FontSubstitution { original: String, substituted: String },
    ColorConverted { from: String, to: String },
    ResolutionReduced { requested: u32, actual: u32 },
    FeatureUnsupported { feature: String, printer: String },
}

impl VerificationReport {
    pub fn assert_all(&self) -> Result<(), VerificationError>;
    pub fn assert_page_count(&self, expected: usize) -> Result<(), VerificationError>;
    pub fn assert_page_size(&self, page: usize, expected: Size<Points>) -> Result<(), VerificationError>;
    pub fn assert_text_baseline(&self, page: usize, text: &str, expected_y: f64, tolerance: f64) -> Result<()>;
    pub fn assert_table_row_count(&self, page: usize, table: usize, expected: usize) -> Result<()>;
}
```
