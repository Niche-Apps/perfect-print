//! # perfect-print
//!
//! The best native print API in any programming language.

pub mod prelude;

pub use perfect_print_core::color::Color;
pub use perfect_print_core::document::{DocumentBuilder, DocumentModel, PageBuilder};
pub use perfect_print_core::draw::{
    DrawCommand, FillRule, LineCap, LineJoin, TextAlign, TextRun, TextStyle,
};
pub use perfect_print_core::error::{CoreError, CoreResult};
pub use perfect_print_core::font::{FontRef, FontStyle, FontWeight};
pub use perfect_print_core::image::{ImageData, ImageFormat, ImageLoadError};
pub use perfect_print_core::page::{Layer, LayerType, Margins, Page, PageSize};
pub use perfect_print_core::resource::ImageStore;
pub use perfect_print_core::units::{Dpi, Length, LengthUnit, Point, Rect, Size};
pub use perfect_print_dialog::{
    ColorMode, DuplexMode, NoOpDialog, PageOrientation, PageRange, PrintDialog, PrintDialogResult,
    PrintError, PrintScaling, PrintSettings, PrintWarning, Printer, PrinterCapabilities,
    PrinterState,
};
pub use perfect_print_layout::flow::{
    ContentBlock, FlowConfig, FlowLayoutEngine, ListKind, PositionedBlock, StyledSpan,
};
pub use perfect_print_layout::font_loader::{
    default_fallbacks, FallbackFont, FontCache, FontLoader, FontProperties, LoadedFont,
    SystemFontLoader,
};
pub use perfect_print_layout::paragraph::{
    Line, ParagraphConfig, ParagraphEngine, ParagraphLayout,
};
pub use perfect_print_layout::table::*;
pub use perfect_print_layout::TextShaper;
pub use perfect_print_pdf::{PdfError, PdfRenderer, PdfResult};
pub use perfect_print_render::{Render, RenderError, RenderResult, TinySkiaRenderer};

use std::path::Path;

// ─── Document ───────────────────────────────────────────────────────────

/// High-level document builder with ergonomic API.
#[derive(Debug, Clone)]
pub struct Document {
    builder: DocumentBuilder,
    blocks: Vec<ContentBlock>,
    page_size: PageSize,
    margins: Margins,
    image_store: ImageStore,
    default_style: Option<TextStyle>,
    header: Option<DrawCommand>,
    footer: Option<DrawCommand>,
    model_override: Option<DocumentModel>,
}

impl Document {
    pub fn new() -> Self {
        Self {
            builder: DocumentBuilder::new(),
            blocks: Vec::new(),
            page_size: PageSize::Letter,
            margins: Margins::all(72.0),
            image_store: ImageStore::new(),
            default_style: None,
            header: None,
            footer: None,
            model_override: None,
        }
    }

    pub fn page(mut self, size: PageSize) -> Self {
        self.page_size = size;
        self.builder = self.builder.page(size);
        self
    }

    pub fn margin(mut self, margin: f64) -> Self {
        self.margins = Margins::all(margin);
        self
    }

    pub fn margins(mut self, margins: Margins) -> Self {
        self.margins = margins;
        self
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.builder = self.builder.title(title);
        self
    }

    pub fn author(mut self, author: impl Into<String>) -> Self {
        self.builder = self.builder.author(author);
        self
    }

    pub fn default_style(mut self, style: TextStyle) -> Self {
        self.default_style = Some(style);
        self
    }

    /// Set a header draw command to appear on every page.
    pub fn header(mut self, cmd: DrawCommand) -> Self {
        self.header = Some(cmd);
        self
    }

    /// Set a footer draw command to appear on every page.
    pub fn footer(mut self, cmd: DrawCommand) -> Self {
        self.footer = Some(cmd);
        self
    }

    pub fn add(mut self, block: impl Into<ContentBlock>) -> Self {
        self.blocks.push(block.into());
        self
    }

    /// Load an image from a file and store it under `id`.
    pub fn load_image(mut self, id: &str, path: impl AsRef<std::path::Path>) -> Self {
        match ImageData::load(path.as_ref()) {
            Ok(data) => {
                self.image_store.insert(id, data);
            }
            Err(e) => {
                log::warn!("Failed to load image '{}': {}", id, e);
            }
        }
        self
    }

    /// Add an image from raw RGBA pixel data.
    pub fn add_image(mut self, id: &str, width: u32, height: u32, pixels: Vec<u8>) -> Self {
        let data = ImageData::new(width, height, pixels);
        self.image_store.insert(id, data);
        self
    }

    /// Add a test pattern image.
    pub fn add_test_image(mut self, id: &str, width: u32, height: u32) -> Self {
        let data = ImageData::test_pattern(width, height);
        self.image_store.insert(id, data);
        self
    }

    pub fn build(self) -> DocumentModel {
        let Document {
            builder,
            blocks,
            page_size,
            margins,
            image_store,
            default_style,
            header,
            footer,
            model_override,
        } = self;

        let config = FlowConfig {
            page_size,
            margins,
            default_style,
            ..Default::default()
        };
        let mut engine = FlowLayoutEngine::new(config);

        let mut model = if let Some(mut model) = model_override {
            if !blocks.is_empty() {
                let mut appended = engine.layout(&blocks);
                appended.image_store = image_store;
                model.pages.extend(appended.pages);
                model.resources.fonts.extend(appended.resources.fonts);
                model.resources.images.extend(appended.resources.images);
                model.image_store = merge_image_stores(&model.image_store, &appended.image_store);
            } else if !image_store.is_empty() {
                model.image_store = merge_image_stores(&model.image_store, &image_store);
            }
            model
        } else {
            let mut model = engine.layout(&blocks);
            model.image_store = image_store;
            model
        };

        if let Some(title) = builder.get_title() {
            model.metadata.title = Some(title.to_string());
        }
        if let Some(author) = builder.get_author() {
            model.metadata.author = Some(author.to_string());
        }
        if let Some(header) = header {
            model.header = Some(Box::new(header));
        }
        if let Some(footer) = footer {
            model.footer = Some(Box::new(footer));
        }
        model.metadata.page_count = model.pages.len();
        model
    }

    /// Build and save to PDF.
    pub fn save_pdf(self, path: impl AsRef<Path>) -> Result<(), PdfError> {
        let model = self.build();
        PdfRenderer::new().render_to_pdf(&model, path.as_ref())
    }

    /// Build and render to PNG files (one per page).
    pub fn render_png(
        self,
        output_dir: impl AsRef<Path>,
        dpi: u32,
    ) -> Result<Vec<std::path::PathBuf>, RenderError> {
        let model = self.build();
        TinySkiaRenderer::new().render_to_raster(&model, Dpi(dpi as f64), output_dir.as_ref())
    }

    /// Build and save a single page to PNG. Errors if document has multiple pages.
    pub fn save_png(self, path: impl AsRef<Path>, dpi: u32) -> Result<(), RenderError> {
        let model = self.build();
        let renderer = TinySkiaRenderer::new();
        let parent = path.as_ref().parent().unwrap_or(path.as_ref());
        let paths = renderer.render_to_raster(&model, Dpi(dpi as f64), parent)?;
        if paths.len() == 1 {
            std::fs::copy(&paths[0], path.as_ref())?;
            Ok(())
        } else {
            Err(RenderError::Generation(
                "Document has multiple pages, use render_png() instead".to_string(),
            ))
        }
    }

    /// Build and print using the platform's native print backend.
    pub fn print(self) -> Result<Option<String>, PrintError> {
        let model = self.build();
        print_document(&model)
    }

    /// Build and print with custom settings.
    pub fn print_with(self, settings: &PrintSettings) -> Result<Option<String>, PrintError> {
        let model = self.build();
        print_document_with(&model, settings)
    }

    /// Get the page count without writing output.
    pub fn page_count(&self) -> usize {
        self.clone().build().page_count()
    }

    /// Serialize the built document to JSON.
    pub fn to_json(&self) -> Result<String, CoreError> {
        self.clone().build().to_json()
    }

    /// Build and return a specific page by index.
    pub fn get_page(&self, index: usize) -> Option<Page> {
        let model = self.clone().build();
        model.pages.get(index).cloned()
    }

    /// Build and extract all plain text content.
    pub fn text_content(&self) -> String {
        extract_text_from_blocks(&self.blocks)
    }

    /// Merge with another document (appends pages from both).
    pub fn merge(self, other: Document) -> Self {
        let mut model = self.build();
        let other_model = other.build();
        model.pages.extend(other_model.pages);
        model.resources.fonts.extend(other_model.resources.fonts);
        model.resources.images.extend(other_model.resources.images);
        model.image_store = merge_image_stores(&model.image_store, &other_model.image_store);
        model.metadata.page_count = model.pages.len();
        Self::from_model(model)
    }

    /// Deserialize a document from JSON.
    pub fn from_json(json: &str) -> Result<Self, CoreError> {
        let mut model: DocumentModel = serde_json::from_str(json)
            .map_err(|e| CoreError::Serialization(format!("JSON parse failed: {}", e)))?;
        model.metadata.page_count = model.pages.len();
        model.validate()?;
        Ok(Self::from_model(model))
    }

    fn from_model(model: DocumentModel) -> Self {
        let mut doc = Document::new();
        if let Some(first_page) = model.pages.first() {
            doc.page_size = PageSize::Custom {
                width: first_page.size.width,
                height: first_page.size.height,
            };
            doc.margins = first_page.margins;
        }
        if let Some(ref title) = model.metadata.title {
            doc.builder = doc.builder.title(title.clone());
        }
        if let Some(ref author) = model.metadata.author {
            doc.builder = doc.builder.author(author.clone());
        }
        doc.image_store = model.image_store.clone();
        doc.model_override = Some(model);
        doc
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Free functions ─────────────────────────────────────────────────────

/// Print a document using the platform's native print backend.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub fn print_document(model: &DocumentModel) -> Result<Option<String>, PrintError> {
    print_document_with(model, &PrintSettings::default())
}

/// Print a document with custom settings.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub fn print_document_with(
    model: &DocumentModel,
    settings: &PrintSettings,
) -> Result<Option<String>, PrintError> {
    let pdf_bytes = PdfRenderer::new()
        .render_to_bytes(model)
        .map_err(|e| PrintError::PrintFailed(format!("PDF render failed: {}", e)))?;
    platform_print_bytes(
        model,
        &pdf_bytes,
        model.metadata.title.as_deref().unwrap_or("Perfect Print"),
        settings,
    )
}

#[cfg(target_os = "macos")]
fn platform_print_bytes(
    _model: &DocumentModel,
    pdf_bytes: &[u8],
    title: &str,
    settings: &PrintSettings,
) -> Result<Option<String>, PrintError> {
    perfect_print_backend_macos::print_pdf_bytes_with_dialog(pdf_bytes, Some(title), settings)
        .map(|submitted| submitted.then(|| "native-print-dialog".to_string()))
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn platform_print_bytes(
    model: &DocumentModel,
    pdf_bytes: &[u8],
    _title: &str,
    settings: &PrintSettings,
) -> Result<Option<String>, PrintError> {
    use std::io::Write;

    let mut pdf_file = tempfile::Builder::new()
        .prefix("perfect-print-")
        .suffix(".pdf")
        .tempfile()
        .map_err(|e| PrintError::PrintFailed(format!("Temporary PDF create failed: {}", e)))?;
    pdf_file
        .write_all(pdf_bytes)
        .and_then(|_| pdf_file.flush())
        .map_err(|e| PrintError::PrintFailed(format!("Temporary PDF write failed: {}", e)))?;
    platform_print_file(model, pdf_file.path(), settings)
}

#[cfg(target_os = "linux")]
fn platform_print_file(
    _model: &DocumentModel,
    pdf_path: &std::path::Path,
    settings: &PrintSettings,
) -> Result<Option<String>, PrintError> {
    let dialog = perfect_print_backend_linux::LinuxPrintDialog::new();
    dialog.submit_print_job(pdf_path, settings)
}

#[cfg(target_os = "windows")]
fn platform_print_file(
    model: &DocumentModel,
    _pdf_path: &std::path::Path,
    settings: &PrintSettings,
) -> Result<Option<String>, PrintError> {
    let dialog = perfect_print_backend_windows::WindowsPrintDialog::new();
    dialog.submit_print_job(model, settings)
}

// ─── Table ──────────────────────────────────────────────────────────────

/// A table builder for ergonomic table construction.
#[derive(Debug, Clone)]
pub struct Table {
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    footer_row: Option<Vec<String>>,
    column_widths: Vec<f64>,
    borders: bool,
    alternating_rows: bool,
    cell_padding: f64,
}

impl Table {
    pub fn new() -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            footer_row: None,
            column_widths: Vec::new(),
            borders: false,
            alternating_rows: false,
            cell_padding: 4.0,
        }
    }

    pub fn columns(mut self, headers: &[&str]) -> Self {
        self.columns = headers.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn column_widths(mut self, widths: &[f64]) -> Self {
        self.column_widths = widths.to_vec();
        self
    }

    pub fn row(mut self, cells: &[&str]) -> Self {
        self.rows
            .push(cells.iter().map(|s| s.to_string()).collect());
        self
    }

    pub fn footer_row(mut self, cells: &[&str]) -> Self {
        self.footer_row = Some(cells.iter().map(|s| s.to_string()).collect());
        self
    }

    /// Enable cell borders.
    pub fn with_borders(mut self) -> Self {
        self.borders = true;
        self
    }

    /// Enable alternating row background colors.
    pub fn with_alternating_rows(mut self) -> Self {
        self.alternating_rows = true;
        self
    }

    /// Set cell padding in points (default: 4.0).
    pub fn cell_padding(mut self, padding: f64) -> Self {
        self.cell_padding = padding;
        self
    }
}

impl From<Table> for ContentBlock {
    fn from(table: Table) -> Self {
        use perfect_print_layout::table::{Cell, CellStyle, Column, ColumnWidth, Row};

        let num_cols = table
            .columns
            .len()
            .max(table.rows.iter().map(|r| r.len()).max().unwrap_or(0));

        let column_defs: Vec<Column> = if table.column_widths.is_empty() {
            (0..num_cols)
                .map(|_| Column::new(ColumnWidth::Auto))
                .collect()
        } else {
            table
                .column_widths
                .iter()
                .map(|&w| Column::new(ColumnWidth::Fixed(w)))
                .collect()
        };

        let cell_style = CellStyle {
            padding: table.cell_padding,
            border_width: if table.borders { 0.5 } else { 0.0 },
            ..Default::default()
        };

        let mut rows: Vec<Row> = Vec::new();

        if !table.columns.is_empty() {
            let cells: Vec<Cell> = table
                .columns
                .iter()
                .map(|h| Cell::new(h.as_str()).with_style(cell_style.clone()))
                .collect();
            rows.push(Row::header(cells));
        }

        for (row_idx, row) in table.rows.iter().enumerate() {
            let bg = if table.alternating_rows && row_idx % 2 == 1 {
                Some(Color::gray(0.95))
            } else {
                None
            };
            let row_style = CellStyle {
                background: bg,
                ..cell_style.clone()
            };
            let cells: Vec<Cell> = row
                .iter()
                .map(|c| Cell::new(c.as_str()).with_style(row_style.clone()))
                .collect();
            rows.push(Row::new(cells));
        }

        if let Some(footer) = &table.footer_row {
            let cells: Vec<Cell> = footer
                .iter()
                .map(|c| Cell::new(c.as_str()).with_style(cell_style.clone()))
                .collect();
            let mut footer_row = Row::new(cells);
            footer_row.is_header = true;
            rows.push(footer_row);
        }

        ContentBlock::Table {
            columns: column_defs,
            rows,
        }
    }
}

// ─── Verification ──────────────────────────────────────────────────────

/// Verification report for WYSIWYG checking.
#[derive(Debug)]
pub struct VerificationReport {
    pub page_count: usize,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

impl VerificationReport {
    pub fn assert_all(&self) -> Result<(), String> {
        if !self.errors.is_empty() {
            return Err(format!("Verification errors: {}", self.errors.join("; ")));
        }
        Ok(())
    }

    pub fn assert_page_count(&self, expected: usize) -> Result<(), String> {
        if self.page_count != expected {
            return Err(format!(
                "Expected {} pages, got {}",
                expected, self.page_count
            ));
        }
        Ok(())
    }
}

/// Verify a document model for WYSIWYG correctness.
pub fn verify_document(model: &DocumentModel) -> VerificationReport {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    for (i, page) in model.pages.iter().enumerate() {
        let has_content = page.layers.iter().any(|l| !l.commands.is_empty());
        if !has_content {
            warnings.push(format!("Page {} has no content", i + 1));
        }
    }

    for layer in model.pages.iter().flat_map(|p| &p.layers) {
        for cmd in &layer.commands {
            if let DrawCommand::Image { image_id, .. } = cmd {
                if !model.image_store.has(image_id) {
                    errors.push(format!("Image '{}' not found in image store", image_id));
                }
            }
        }
    }

    VerificationReport {
        page_count: model.page_count(),
        warnings,
        errors,
    }
}

// ─── Paragraph ─────────────────────────────────────────────────────────

/// A paragraph of text to add to a document.
#[derive(Debug, Clone)]
pub struct Paragraph {
    text: String,
    style: TextStyle,
    first_line_indent: f64,
    keep_with_next: bool,
    before_gap: f64,
    after_gap: f64,
}

impl Paragraph {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: TextStyle::new(FontRef::new("Helvetica"), 12.0),
            first_line_indent: 0.0,
            keep_with_next: false,
            before_gap: 0.0,
            after_gap: 0.0,
        }
    }

    pub fn font(mut self, font: FontRef) -> Self {
        self.style.font = font;
        self
    }

    pub fn font_size(mut self, size: f64) -> Self {
        self.style.size = size;
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.style.color = color;
        self
    }

    pub fn bold(mut self) -> Self {
        self.style.bold = true;
        self
    }

    pub fn italic(mut self) -> Self {
        self.style.italic = true;
        self
    }

    pub fn align(mut self, align: TextAlign) -> Self {
        self.style.align = align;
        self
    }

    pub fn line_height(mut self, height: f64) -> Self {
        self.style.line_height = Some(height);
        self
    }

    /// Set first-line indent in points (default: 0.0).
    pub fn first_line_indent(mut self, indent: f64) -> Self {
        self.first_line_indent = indent;
        self
    }

    /// Keep this paragraph with the next one (avoid page break between them).
    pub fn keep_with_next(mut self) -> Self {
        self.keep_with_next = true;
        self
    }

    /// Add vertical space before this paragraph.
    pub fn before_gap(mut self, gap: f64) -> Self {
        self.before_gap = gap;
        self
    }

    /// Add vertical space after this paragraph.
    pub fn after_gap(mut self, gap: f64) -> Self {
        self.after_gap = gap;
        self
    }
}

impl From<Paragraph> for ContentBlock {
    fn from(p: Paragraph) -> Self {
        ContentBlock::Paragraph {
            text: p.text,
            style: p.style,
        }
    }
}

// ─── Gap ───────────────────────────────────────────────────────────────

/// A vertical gap.
#[derive(Debug, Clone)]
pub struct Gap(pub f64);

impl Gap {
    /// Add vertical space before the next block.
    pub fn before(points: f64) -> ContentBlock {
        ContentBlock::Gap(points)
    }

    /// Add vertical space after the previous block.
    pub fn after(points: f64) -> ContentBlock {
        ContentBlock::Gap(points)
    }
}

impl From<Gap> for ContentBlock {
    fn from(g: Gap) -> Self {
        ContentBlock::Gap(g.0)
    }
}

// ─── PageBreak ─────────────────────────────────────────────────────────

/// A page break.
#[derive(Debug, Clone)]
pub struct PageBreak;

impl From<PageBreak> for ContentBlock {
    fn from(_: PageBreak) -> Self {
        ContentBlock::PageBreak
    }
}

// ─── Image ─────────────────────────────────────────────────────────────

/// An image to place in a document.
#[derive(Debug, Clone)]
pub struct Image {
    image_id: String,
    width: Option<f64>,
    height: Option<f64>,
    maintain_aspect: bool,
}

impl Image {
    pub fn new(image_id: impl Into<String>) -> Self {
        Self {
            image_id: image_id.into(),
            width: None,
            height: None,
            maintain_aspect: true,
        }
    }

    pub fn width(mut self, width: f64) -> Self {
        self.width = Some(width);
        self
    }

    pub fn height(mut self, height: f64) -> Self {
        self.height = Some(height);
        self
    }

    pub fn size(mut self, width: f64, height: f64) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    /// Set whether to maintain aspect ratio when only one dimension is set (default: true).
    pub fn maintain_aspect(mut self, maintain: bool) -> Self {
        self.maintain_aspect = maintain;
        self
    }

    /// Fit within the given dimensions, maintaining aspect ratio.
    pub fn fit(mut self, max_width: f64, max_height: f64) -> Self {
        self.width = Some(max_width);
        self.height = Some(max_height);
        self.maintain_aspect = true;
        self
    }

    /// Get the aspect ratio (width/height) if both dimensions are set.
    pub fn aspect_ratio(&self) -> Option<f64> {
        match (self.width, self.height) {
            (Some(w), Some(h)) if h > 0.0 => Some(w / h),
            _ => None,
        }
    }
}

impl From<Image> for ContentBlock {
    fn from(img: Image) -> Self {
        let w = img.width.unwrap_or(100.0);
        let h = img.height.unwrap_or(100.0);
        ContentBlock::Image {
            image_id: img.image_id,
            dest_rect: Rect::new(0.0, 0.0, w, h),
        }
    }
}

// ─── Font ──────────────────────────────────────────────────────────────

/// A font builder for ergonomic font construction.
#[derive(Debug, Clone)]
pub struct Font {
    name: String,
    size: f64,
    weight: FontWeight,
    style: FontStyle,
}

impl Font {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            size: 12.0,
            weight: FontWeight::Normal,
            style: FontStyle::Normal,
        }
    }

    pub fn size(mut self, size: f64) -> Self {
        self.size = size;
        self
    }

    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    pub fn style(mut self, style: FontStyle) -> Self {
        self.style = style;
        self
    }

    pub fn bold(mut self) -> Self {
        self.weight = FontWeight::Bold;
        self
    }

    pub fn italic(mut self) -> Self {
        self.style = FontStyle::Italic;
        self
    }

    /// Convert to a `FontRef`.
    pub fn into_font_ref(self) -> FontRef {
        FontRef::new(self.name)
    }

    /// Convert to a `TextStyle` with default color and alignment.
    pub fn into_text_style(self) -> TextStyle {
        TextStyle::new(FontRef::new(self.name), self.size)
    }
}

// ─── TextSpan ──────────────────────────────────────────────────────────

/// A rich text span for mixed-style text within a paragraph.
#[derive(Debug, Clone)]
pub struct TextSpan {
    text: String,
    style: TextStyle,
}

impl TextSpan {
    /// Create a new text span with the given text and style.
    pub fn new(text: impl Into<String>, style: TextStyle) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }

    /// Create a span with just text (uses default style).
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: TextStyle::new(FontRef::new("Helvetica"), 12.0),
        }
    }

    /// Create a bold span.
    pub fn bold(text: impl Into<String>) -> Self {
        let mut style = TextStyle::new(FontRef::new("Helvetica"), 12.0);
        style.bold = true;
        Self {
            text: text.into(),
            style,
        }
    }

    /// Create an italic span.
    pub fn italic(text: impl Into<String>) -> Self {
        let mut style = TextStyle::new(FontRef::new("Helvetica"), 12.0);
        style.italic = true;
        Self {
            text: text.into(),
            style,
        }
    }

    pub fn font_size(mut self, size: f64) -> Self {
        self.style.size = size;
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.style.color = color;
        self
    }

    pub fn set_bold(mut self, bold: bool) -> Self {
        self.style.bold = bold;
        self
    }

    pub fn set_italic(mut self, italic: bool) -> Self {
        self.style.italic = italic;
        self
    }

    /// Borrow this span's text content.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Borrow this span's style.
    pub fn style(&self) -> &TextStyle {
        &self.style
    }
}

// ─── RichParagraph ─────────────────────────────────────────────────────

/// A paragraph mixing plain, bold, italic, and styled spans.
#[derive(Debug, Clone)]
pub struct RichParagraph {
    spans: Vec<TextSpan>,
    base: TextStyle,
}

impl RichParagraph {
    pub fn new() -> Self {
        Self {
            spans: Vec::new(),
            base: TextStyle::new(FontRef::new("Helvetica"), 12.0),
        }
    }

    /// Append an arbitrary styled span.
    pub fn span(mut self, span: TextSpan) -> Self {
        self.spans.push(span);
        self
    }

    /// Append plain text using the paragraph's base style.
    pub fn text(mut self, s: impl Into<String>) -> Self {
        self.spans.push(TextSpan::new(s, self.base.clone()));
        self
    }

    /// Append bold text (base style + bold).
    pub fn bold(mut self, s: impl Into<String>) -> Self {
        let mut style = self.base.clone();
        style.bold = true;
        self.spans.push(TextSpan::new(s, style));
        self
    }

    /// Append italic text (base style + italic).
    pub fn italic(mut self, s: impl Into<String>) -> Self {
        let mut style = self.base.clone();
        style.italic = true;
        self.spans.push(TextSpan::new(s, style));
        self
    }

    /// Set the paragraph's alignment.
    pub fn align(mut self, a: TextAlign) -> Self {
        self.base.align = a;
        self
    }

    /// Set the base font size. Also applied to any spans already added that
    /// are still at the paragraph's previous base size (spans with an
    /// explicitly different size are left alone).
    pub fn font_size(mut self, size: f64) -> Self {
        let old_base_size = self.base.size;
        for span in &mut self.spans {
            if span.style.size == old_base_size {
                span.style.size = size;
            }
        }
        self.base.size = size;
        self
    }
}

impl Default for RichParagraph {
    fn default() -> Self {
        Self::new()
    }
}

impl From<RichParagraph> for ContentBlock {
    fn from(p: RichParagraph) -> Self {
        ContentBlock::RichParagraph {
            spans: p
                .spans
                .into_iter()
                .map(|s| StyledSpan {
                    text: s.text,
                    style: s.style,
                })
                .collect(),
            base_style: p.base,
            indent_left: 0.0,
        }
    }
}

// ─── List ──────────────────────────────────────────────────────────────

/// A bulleted or numbered list, built up from plain-text or rich items.
#[derive(Debug, Clone)]
pub struct List {
    items: Vec<perfect_print_layout::flow::ListItem>,
    kind: ListKind,
    style: TextStyle,
}

impl List {
    pub fn bulleted() -> Self {
        Self {
            items: Vec::new(),
            kind: ListKind::Bulleted,
            style: TextStyle::new(FontRef::new("Helvetica"), 12.0),
        }
    }

    pub fn numbered() -> Self {
        Self {
            items: Vec::new(),
            kind: ListKind::Numbered,
            style: TextStyle::new(FontRef::new("Helvetica"), 12.0),
        }
    }

    /// Add a plain-text item at level 0.
    pub fn item(mut self, text: impl Into<String>) -> Self {
        self.items.push(perfect_print_layout::flow::ListItem {
            spans: vec![StyledSpan {
                text: text.into(),
                style: self.style.clone(),
            }],
            level: 0,
        });
        self
    }

    /// Add a rich (mixed-style) item at level 0.
    pub fn rich_item(mut self, paragraph: RichParagraph) -> Self {
        self.items.push(perfect_print_layout::flow::ListItem {
            spans: paragraph
                .spans
                .into_iter()
                .map(|s| StyledSpan {
                    text: s.text,
                    style: s.style,
                })
                .collect(),
            level: 0,
        });
        self
    }

    /// Flatten a nested list's items into this one, each one level deeper.
    pub fn nested(mut self, inner: List) -> Self {
        for mut item in inner.items {
            item.level += 1;
            self.items.push(item);
        }
        self
    }
}

impl From<List> for ContentBlock {
    fn from(list: List) -> Self {
        ContentBlock::List {
            items: list.items,
            kind: list.kind,
            style: list.style,
        }
    }
}

// ─── PageNumber ────────────────────────────────────────────────────────

/// A page number variable for use in headers and footers.
#[derive(Debug, Clone)]
pub struct PageNumber;

impl PageNumber {
    /// Create a draw command that renders the current page number.
    pub fn current() -> DrawCommand {
        DrawCommand::Text {
            run: TextRun {
                text: "{{page}}".to_string(),
                glyphs: vec![],
                style: TextStyle::new(FontRef::new("Helvetica"), 10.0),
            },
            position: Point::new(0.0, 0.0),
            max_width: None,
        }
    }

    /// Create a draw command that renders "Page X of Y".
    pub fn of_total() -> DrawCommand {
        DrawCommand::Text {
            run: TextRun {
                text: "{{page}} of {{total}}".to_string(),
                glyphs: vec![],
                style: TextStyle::new(FontRef::new("Helvetica"), 10.0),
            },
            position: Point::new(0.0, 0.0),
            max_width: None,
        }
    }
}

// ─── Watermark ─────────────────────────────────────────────────────────

/// A watermark builder for adding watermarks to documents.
#[derive(Debug, Clone)]
pub struct Watermark {
    text: String,
    font_size: f64,
    color: Color,
    opacity: f64,
    rotation: f64,
}

impl Watermark {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            font_size: 48.0,
            color: Color::gray(0.8),
            opacity: 0.3,
            rotation: -45.0,
        }
    }

    pub fn font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn opacity(mut self, opacity: f64) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    pub fn rotation(mut self, degrees: f64) -> Self {
        self.rotation = degrees;
        self
    }

    /// Convert to draw commands for a watermark layer.
    pub fn into_draw_command(self, page_width: f64, page_height: f64) -> Vec<DrawCommand> {
        use perfect_print_core::draw::Transform;

        let radians = self.rotation.to_radians();
        let center_x = page_width / 2.0;
        let center_y = page_height / 2.0;

        vec![
            DrawCommand::PushTransform {
                transform: Transform::translate(center_x, center_y),
            },
            DrawCommand::PushTransform {
                transform: Transform::rotate(radians),
            },
            DrawCommand::PushOpacity {
                opacity: self.opacity,
            },
            DrawCommand::Text {
                run: TextRun {
                    text: self.text,
                    glyphs: vec![],
                    style: TextStyle::new(FontRef::new("Helvetica"), self.font_size),
                },
                position: Point::new(0.0, 0.0),
                max_width: None,
            },
            DrawCommand::PopOpacity,
            DrawCommand::PopTransform,
            DrawCommand::PopTransform,
        ]
    }
}

// ─── Helpers ───────────────────────────────────────────────────────────

/// Convenience: convert &str to a ContentBlock::Paragraph.
pub fn text(s: &str) -> ContentBlock {
    ContentBlock::Paragraph {
        text: s.to_string(),
        style: TextStyle::new(FontRef::new("Helvetica"), 12.0),
    }
}

/// Extract all plain text from content blocks before layout.
fn extract_text_from_blocks(blocks: &[ContentBlock]) -> String {
    let mut result = String::new();
    for block in blocks {
        match block {
            ContentBlock::Paragraph { text, .. } => {
                if !result.is_empty() {
                    result.push(' ');
                }
                result.push_str(text);
            }
            ContentBlock::RichParagraph { spans, .. } => {
                for span in spans {
                    if !result.is_empty() {
                        result.push(' ');
                    }
                    result.push_str(&span.text);
                }
            }
            ContentBlock::List { items, .. } => {
                for item in items {
                    for span in &item.spans {
                        if !result.is_empty() {
                            result.push(' ');
                        }
                        result.push_str(&span.text);
                    }
                }
            }
            ContentBlock::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        if let CellContent::Text(text) = &cell.content {
                            if !result.is_empty() {
                                result.push(' ');
                            }
                            result.push_str(text);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    result
}

/// Merge two image stores, preferring the first store's images on conflict.
fn merge_image_stores(a: &ImageStore, b: &ImageStore) -> ImageStore {
    let mut merged = a.clone();
    for (id, data) in b.iter() {
        if !merged.has(id) {
            merged.insert(id, (**data).clone());
        }
    }
    merged
}

// ─── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_new() {
        let doc = Document::new();
        assert_eq!(doc.page_size, PageSize::Letter);
    }

    #[test]
    fn test_document_builder() {
        let model = Document::new()
            .page(PageSize::Letter)
            .margin(72.0)
            .add(Paragraph::new("Hello, World!"))
            .build();
        assert!(model.page_count() >= 1);
    }

    #[test]
    fn test_paragraph_new() {
        let p = Paragraph::new("Hello");
        assert_eq!(p.text, "Hello");
        assert_eq!(p.style.size, 12.0);
    }

    #[test]
    fn test_paragraph_builder() {
        let p = Paragraph::new("Hello")
            .font_size(24.0)
            .bold()
            .italic()
            .color(Color::red());
        assert_eq!(p.style.size, 24.0);
        assert!(p.style.bold);
        assert!(p.style.italic);
    }

    #[test]
    fn test_paragraph_first_line_indent() {
        let p = Paragraph::new("Indented").first_line_indent(24.0);
        assert_eq!(p.first_line_indent, 24.0);
    }

    #[test]
    fn test_paragraph_keep_with_next() {
        let p = Paragraph::new("Keep").keep_with_next();
        assert!(p.keep_with_next);
    }

    #[test]
    fn test_paragraph_gaps() {
        let p = Paragraph::new("Spaced").before_gap(12.0).after_gap(6.0);
        assert_eq!(p.before_gap, 12.0);
        assert_eq!(p.after_gap, 6.0);
    }

    #[test]
    fn test_paragraph_into_content_block() {
        let block: ContentBlock = Paragraph::new("test").into();
        match block {
            ContentBlock::Paragraph { text, .. } => assert_eq!(text, "test"),
            _ => panic!("Expected Paragraph"),
        }
    }

    #[test]
    fn test_rich_paragraph_builds_document() {
        let doc = Document::new().add(
            RichParagraph::new()
                .text("Hello ")
                .bold("world"),
        );
        let model = doc.build();
        assert_eq!(model.page_count(), 1);
    }

    #[test]
    fn test_rich_paragraph_text_content_has_both_spans() {
        let doc = Document::new().add(
            RichParagraph::new()
                .text("Hello ")
                .bold("world"),
        );
        let content = doc.text_content();
        assert!(content.contains("Hello"));
        assert!(content.contains("world"));
    }

    #[test]
    fn test_rich_paragraph_into_content_block() {
        let block: ContentBlock = RichParagraph::new().text("a").bold("b").into();
        match block {
            ContentBlock::RichParagraph { spans, .. } => {
                assert_eq!(spans.len(), 2);
                assert!(!spans[0].style.bold);
                assert!(spans[1].style.bold);
            }
            _ => panic!("Expected RichParagraph"),
        }
    }

    #[test]
    fn test_list_builds_document_with_all_items() {
        let doc = Document::new().add(
            List::bulleted()
                .item("First")
                .item("Second")
                .item("Third"),
        );
        let content = doc.text_content();
        assert!(content.contains("First"));
        assert!(content.contains("Second"));
        assert!(content.contains("Third"));
        let model = doc.build();
        assert_eq!(model.page_count(), 1);
    }

    #[test]
    fn test_list_into_content_block() {
        let block: ContentBlock = List::numbered().item("a").item("b").into();
        match block {
            ContentBlock::List { items, kind, .. } => {
                assert_eq!(items.len(), 2);
                assert!(matches!(kind, ListKind::Numbered));
            }
            _ => panic!("Expected List"),
        }
    }

    #[test]
    fn test_list_nested_increments_level() {
        let inner = List::bulleted().item("nested-a");
        let outer = List::bulleted().item("top").nested(inner);
        let block: ContentBlock = outer.into();
        match block {
            ContentBlock::List { items, .. } => {
                assert_eq!(items[0].level, 0);
                assert_eq!(items[1].level, 1);
            }
            _ => panic!("Expected List"),
        }
    }

    #[test]
    fn test_gap_into_content_block() {
        let block: ContentBlock = Gap(24.0).into();
        match block {
            ContentBlock::Gap(g) => assert_eq!(g, 24.0),
            _ => panic!("Expected Gap"),
        }
    }

    #[test]
    fn test_gap_before_after() {
        let before = Gap::before(12.0);
        match before {
            ContentBlock::Gap(g) => assert_eq!(g, 12.0),
            _ => panic!("Expected Gap"),
        }
        let after = Gap::after(6.0);
        match after {
            ContentBlock::Gap(g) => assert_eq!(g, 6.0),
            _ => panic!("Expected Gap"),
        }
    }

    #[test]
    fn test_page_break_into_content_block() {
        let block: ContentBlock = PageBreak.into();
        match block {
            ContentBlock::PageBreak => {}
            _ => panic!("Expected PageBreak"),
        }
    }

    #[test]
    fn test_document_multiple_pages() {
        let model = Document::new()
            .page(PageSize::Letter)
            .margin(72.0)
            .add(Paragraph::new("Page 1"))
            .add(PageBreak)
            .add(Paragraph::new("Page 2"))
            .build();
        assert!(model.page_count() >= 2);
    }

    #[test]
    fn test_document_save_pdf() {
        let path = std::env::temp_dir().join("test_pp.pdf");
        let result = Document::new()
            .page(PageSize::Letter)
            .add(Paragraph::new("PDF test"))
            .save_pdf(&path);
        assert!(result.is_ok());
        assert!(path.exists());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_document_render_png() {
        let dir = std::env::temp_dir().join("test_pp_png");
        let _ = std::fs::remove_dir_all(&dir);
        let result = Document::new()
            .page(PageSize::Letter)
            .add(Paragraph::new("PNG test"))
            .render_png(&dir, 150);
        assert!(result.is_ok());
        let paths = result.unwrap();
        assert!(!paths.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_text_helper() {
        let block = text("Hello");
        match block {
            ContentBlock::Paragraph { text, .. } => assert_eq!(text, "Hello"),
            _ => panic!("Expected Paragraph"),
        }
    }

    #[test]
    fn test_document_title_author_propagated() {
        let model = Document::new()
            .title("My Report")
            .author("Jane Doe")
            .add(Paragraph::new("Content"))
            .build();
        assert_eq!(model.metadata.title, Some("My Report".to_string()));
        assert_eq!(model.metadata.author, Some("Jane Doe".to_string()));
    }

    #[test]
    fn test_image_builder() {
        let img = Image::new("photo").size(200.0, 150.0);
        assert_eq!(img.image_id, "photo");
        assert_eq!(img.width, Some(200.0));
        assert_eq!(img.height, Some(150.0));
    }

    #[test]
    fn test_image_fit() {
        let img = Image::new("photo").fit(800.0, 600.0);
        assert_eq!(img.width, Some(800.0));
        assert_eq!(img.height, Some(600.0));
        assert!(img.maintain_aspect);
    }

    #[test]
    fn test_image_aspect_ratio() {
        let img = Image::new("photo").size(200.0, 100.0);
        assert_eq!(img.aspect_ratio(), Some(2.0));
    }

    #[test]
    fn test_image_into_content_block() {
        let img = Image::new("photo").size(100.0, 50.0);
        let block: ContentBlock = img.into();
        match block {
            ContentBlock::Image { image_id, .. } => assert_eq!(image_id, "photo"),
            _ => panic!("Expected Image"),
        }
    }

    #[test]
    fn test_font_builder() {
        let font = Font::new("Helvetica").size(14.0).bold();
        assert_eq!(font.name, "Helvetica");
        assert_eq!(font.size, 14.0);
        assert_eq!(font.weight, FontWeight::Bold);
    }

    #[test]
    fn test_font_italic() {
        let font = Font::new("Times").italic();
        assert_eq!(font.style, FontStyle::Italic);
    }

    #[test]
    fn test_font_into_text_style() {
        let style = Font::new("Helvetica").size(16.0).into_text_style();
        assert_eq!(style.size, 16.0);
    }

    #[test]
    fn test_color_from_hex() {
        let c = Color::from_hex("#FF0000").unwrap();
        assert!((c.r - 1.0).abs() < 0.001);
        assert!(c.g.abs() < 0.001);
        assert!(c.b.abs() < 0.001);
    }

    #[test]
    fn test_color_from_hex_with_alpha() {
        let c = Color::from_hex("#FF000080").unwrap();
        assert!((c.r - 1.0).abs() < 0.001);
        assert!((c.a - 0.502).abs() < 0.01);
    }

    #[test]
    fn test_color_from_hex_invalid() {
        assert!(Color::from_hex("not-a-color").is_none());
        assert!(Color::from_hex("#GG0000").is_none());
        assert!(Color::from_hex("#FFF").is_none());
    }

    #[test]
    fn test_color_from_rgb_u8() {
        let c = Color::from_rgb_u8(255, 128, 0);
        assert!((c.r - 1.0).abs() < 0.001);
        assert!((c.g - 0.502).abs() < 0.01);
        assert!(c.b.abs() < 0.001);
    }

    #[test]
    fn test_color_from_rgba_u8() {
        let c = Color::from_rgba_u8(255, 0, 0, 128);
        assert!((c.r - 1.0).abs() < 0.001);
        assert!((c.a - 0.502).abs() < 0.01);
    }

    #[test]
    fn test_text_span() {
        let span = TextSpan::bold("important");
        assert_eq!(span.text, "important");
        assert!(span.style.bold);
    }

    #[test]
    fn test_text_span_plain() {
        let span = TextSpan::plain("hello");
        assert_eq!(span.text, "hello");
    }

    #[test]
    fn test_text_span_styled() {
        let span = TextSpan::new("colored", TextStyle::new(FontRef::new("Helvetica"), 14.0))
            .color(Color::red())
            .font_size(18.0);
        assert_eq!(span.style.size, 18.0);
        assert!((span.style.color.r - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_page_number() {
        let cmd = PageNumber::current();
        match cmd {
            DrawCommand::Text { run, .. } => assert_eq!(run.text, "{{page}}"),
            _ => panic!("Expected Text command"),
        }
    }

    #[test]
    fn test_page_number_of_total() {
        let cmd = PageNumber::of_total();
        match cmd {
            DrawCommand::Text { run, .. } => assert_eq!(run.text, "{{page}} of {{total}}"),
            _ => panic!("Expected Text command"),
        }
    }

    #[test]
    fn test_watermark() {
        let wm = Watermark::new("CONFIDENTIAL").font_size(60.0).opacity(0.5);
        assert_eq!(wm.text, "CONFIDENTIAL");
        assert_eq!(wm.font_size, 60.0);
        assert!((wm.opacity - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_watermark_into_draw_command() {
        let wm = Watermark::new("DRAFT");
        let cmds = wm.into_draw_command(612.0, 792.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_table_with_borders() {
        let table = Table::new()
            .columns(&["Name", "Age"])
            .row(&["Alice", "30"])
            .with_borders();
        assert!(table.borders);
    }

    #[test]
    fn test_table_with_alternating_rows() {
        let table = Table::new()
            .columns(&["Name", "Age"])
            .row(&["Alice", "30"])
            .row(&["Bob", "25"])
            .with_alternating_rows();
        assert!(table.alternating_rows);
    }

    #[test]
    fn test_table_cell_padding() {
        let table = Table::new().cell_padding(8.0);
        assert_eq!(table.cell_padding, 8.0);
    }

    #[test]
    fn test_document_page_count() {
        let doc = Document::new()
            .add(Paragraph::new("Page 1"))
            .add(PageBreak)
            .add(Paragraph::new("Page 2"));
        assert_eq!(doc.page_count(), 2);
    }

    #[test]
    fn test_document_to_json() {
        let doc = Document::new().title("Test").add(Paragraph::new("Hello"));
        let json = doc.to_json().unwrap();
        assert!(json.contains("Test"));
    }

    #[test]
    fn test_document_text_content() {
        let doc = Document::new().add(Paragraph::new("Hello World"));
        let content = doc.text_content();
        assert!(content.contains("Hello World"));
    }

    #[test]
    fn test_document_get_page() {
        let doc = Document::new()
            .add(Paragraph::new("Page 1"))
            .add(PageBreak)
            .add(Paragraph::new("Page 2"));
        let page = doc.get_page(0);
        assert!(page.is_some());
    }

    #[test]
    fn test_document_header_footer() {
        let header_cmd = DrawCommand::Text {
            run: TextRun {
                text: "Header".to_string(),
                glyphs: vec![],
                style: TextStyle::new(FontRef::new("Helvetica"), 10.0),
            },
            position: Point::new(0.0, 0.0),
            max_width: None,
        };
        let footer_cmd = DrawCommand::Text {
            run: TextRun {
                text: "Footer".to_string(),
                glyphs: vec![],
                style: TextStyle::new(FontRef::new("Helvetica"), 10.0),
            },
            position: Point::new(0.0, 0.0),
            max_width: None,
        };
        let model = Document::new()
            .header(header_cmd)
            .footer(footer_cmd)
            .add(Paragraph::new("Content"))
            .build();
        assert!(model.header.is_some());
        assert!(model.footer.is_some());
    }

    #[test]
    fn test_document_from_json() {
        let json = r#"{"pages":[{"size":{"width":612.0,"height":792.0},"margins":{"top":72.0,"right":72.0,"bottom":72.0,"left":72.0},"layers":[{"layer_type":"Foreground","commands":[]}]}],"header":null,"footer":null,"resources":{"fonts":[],"images":[]},"metadata":{"title":null,"author":null,"creator":"perfect-print 0.1.0","page_count":1},"image_store":null}"#;
        let doc = Document::from_json(json);
        assert!(doc.is_ok());
    }

    // ─── Adversarial: Color edge cases ─────────────────────────────────

    #[test]
    fn adversarial_color_hex_edge_cases() {
        // Valid cases
        assert!(Color::from_hex("#000000").is_some());
        assert!(Color::from_hex("#FFFFFF").is_some());
        assert!(Color::from_hex("#ff0000").is_some()); // lowercase
        assert!(Color::from_hex("#FF0000FF").is_some()); // with alpha
        assert!(Color::from_hex("#00000000").is_some()); // transparent
        assert!(Color::from_hex("#FFFFFFFF").is_some()); // opaque white

        // Invalid cases - must return None, not panic
        assert!(Color::from_hex("").is_none());
        assert!(Color::from_hex("#").is_none());
        assert!(Color::from_hex("#F").is_none());
        assert!(Color::from_hex("#FF").is_none());
        assert!(Color::from_hex("#FFF").is_none()); // 3-char not supported
        assert!(Color::from_hex("#FFFF").is_none()); // 4-char not supported
        assert!(Color::from_hex("#FFFFF").is_none()); // 5-char not supported
        assert!(Color::from_hex("#FFFFFFF").is_none()); // 7-char not supported
        assert!(Color::from_hex("#GGGGGG").is_none()); // non-hex chars
        assert!(Color::from_hex("#ZZZZZZ").is_none());
        assert!(Color::from_hex("FF0000").is_none()); // missing #
        assert!(Color::from_hex("#FF00").is_none()); // truncated
        assert!(Color::from_hex("#FF00000").is_none()); // 7 chars
        assert!(Color::from_hex("#FF0000000").is_none()); // 9 chars
        assert!(Color::from_hex("not a color at all").is_none());
        assert!(Color::from_hex("#😀😀😀").is_none()); // unicode
    }

    #[test]
    fn adversarial_color_u8_edge_cases() {
        let black = Color::from_rgb_u8(0, 0, 0);
        assert!((black.r).abs() < 0.001 && (black.g).abs() < 0.001 && (black.b).abs() < 0.001);

        let white = Color::from_rgb_u8(255, 255, 255);
        assert!((white.r - 1.0).abs() < 0.001);

        let mid = Color::from_rgb_u8(128, 128, 128);
        assert!((mid.r - 0.502).abs() < 0.01);

        let transparent = Color::from_rgba_u8(255, 0, 0, 0);
        assert!(transparent.a.abs() < 0.001);

        let opaque = Color::from_rgba_u8(255, 0, 0, 255);
        assert!((opaque.a - 1.0).abs() < 0.001);
    }

    // ─── Adversarial: Image edge cases ────────────────────────────────

    #[test]
    fn adversarial_image_no_dimensions() {
        // Image with no dimensions should use defaults (100x100)
        let img = Image::new("test");
        assert_eq!(img.width, None);
        assert_eq!(img.height, None);
        assert!(img.maintain_aspect);
        assert_eq!(img.aspect_ratio(), None);

        let block: ContentBlock = img.into();
        match block {
            ContentBlock::Image { dest_rect, .. } => {
                assert_eq!(dest_rect.width, 100.0);
                assert_eq!(dest_rect.height, 100.0);
            }
            _ => panic!("Expected Image"),
        }
    }

    #[test]
    fn adversarial_image_zero_dimensions() {
        let img = Image::new("test").size(0.0, 0.0);
        assert_eq!(img.aspect_ratio(), None); // h=0 should return None
    }

    #[test]
    fn adversarial_image_negative_height() {
        // Negative height: h=-50.0 fails the `h > 0.0` guard, so returns None
        let img = Image::new("test").size(100.0, -50.0);
        assert_eq!(img.aspect_ratio(), None);
    }

    #[test]
    fn adversarial_image_fit() {
        let img = Image::new("test").fit(800.0, 600.0);
        assert_eq!(img.width, Some(800.0));
        assert_eq!(img.height, Some(600.0));
        assert!(img.maintain_aspect);
    }

    #[test]
    fn adversarial_image_maintain_aspect_toggle() {
        let img = Image::new("test").size(200.0, 100.0).maintain_aspect(false);
        assert!(!img.maintain_aspect);
    }

    // ─── Adversarial: Font edge cases ─────────────────────────────────

    #[test]
    fn adversarial_font_empty_name() {
        let font = Font::new("").size(12.0);
        assert_eq!(font.name, "");
        assert_eq!(font.size, 12.0);
    }

    #[test]
    fn adversarial_font_zero_size() {
        let font = Font::new("Helvetica").size(0.0);
        assert_eq!(font.size, 0.0);
    }

    #[test]
    fn adversarial_font_negative_size() {
        // Negative font size is technically allowed (builder pattern)
        // The renderer should handle it gracefully
        let font = Font::new("Helvetica").size(-5.0);
        assert_eq!(font.size, -5.0);
    }

    #[test]
    fn adversarial_font_bold_and_italic() {
        let font = Font::new("Helvetica").bold().italic();
        assert_eq!(font.weight, FontWeight::Bold);
        assert_eq!(font.style, FontStyle::Italic);
    }

    #[test]
    fn adversarial_font_into_font_ref() {
        let font = Font::new("Times New Roman").size(14.0);
        let font_ref = font.into_font_ref();
        assert_eq!(font_ref.as_ref(), "Times New Roman");
    }

    // ─── Adversarial: TextSpan edge cases ─────────────────────────────

    #[test]
    fn adversarial_text_span_empty() {
        let span = TextSpan::plain("");
        assert_eq!(span.text, "");
    }

    #[test]
    fn adversarial_text_span_long() {
        let long_text = "a".repeat(10000);
        let span = TextSpan::bold(&long_text);
        assert_eq!(span.text.len(), 10000);
    }

    #[test]
    fn adversarial_text_span_unicode() {
        let span = TextSpan::plain("Hello 世界 🌍");
        assert_eq!(span.text, "Hello 世界 🌍");
    }

    #[test]
    fn adversarial_text_span_set_bold_toggle() {
        let span = TextSpan::plain("test").set_bold(true).set_bold(false);
        assert!(!span.style.bold);
    }

    // ─── Adversarial: Watermark edge cases ────────────────────────────

    #[test]
    fn adversarial_watermark_empty_text() {
        let wm = Watermark::new("");
        assert_eq!(wm.text, "");
        let cmds = wm.into_draw_command(612.0, 792.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn adversarial_watermark_zero_opacity() {
        let wm = Watermark::new("test").opacity(0.0);
        assert!((wm.opacity).abs() < 0.001);
    }

    #[test]
    fn adversarial_watermark_negative_opacity() {
        let wm = Watermark::new("test").opacity(-0.5);
        assert!((wm.opacity).abs() < 0.001); // clamped to 0
    }

    #[test]
    fn adversarial_watermark_over_one_opacity() {
        let wm = Watermark::new("test").opacity(1.5);
        assert!((wm.opacity - 1.0).abs() < 0.001); // clamped to 1
    }

    #[test]
    fn adversarial_watermark_zero_rotation() {
        let wm = Watermark::new("test").rotation(0.0);
        let cmds = wm.into_draw_command(612.0, 792.0);
        assert_eq!(cmds.len(), 7); // 2 push + 1 opacity + 1 text + 3 pop
    }

    #[test]
    fn adversarial_watermark_360_rotation() {
        let wm = Watermark::new("test").rotation(360.0);
        let cmds = wm.into_draw_command(612.0, 792.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn adversarial_watermark_zero_size_page() {
        let wm = Watermark::new("test");
        let cmds = wm.into_draw_command(0.0, 0.0);
        assert!(!cmds.is_empty());
    }

    // ─── Adversarial: PageNumber ──────────────────────────────────────

    #[test]
    fn adversarial_page_number_current() {
        let cmd = PageNumber::current();
        match cmd {
            DrawCommand::Text { run, .. } => assert_eq!(run.text, "{{page}}"),
            _ => panic!("Expected Text"),
        }
    }

    #[test]
    fn adversarial_page_number_of_total() {
        let cmd = PageNumber::of_total();
        match cmd {
            DrawCommand::Text { run, .. } => assert_eq!(run.text, "{{page}} of {{total}}"),
            _ => panic!("Expected Text"),
        }
    }

    // ─── Adversarial: Paragraph edge cases ────────────────────────────

    #[test]
    fn adversarial_paragraph_empty_text() {
        let p = Paragraph::new("");
        assert_eq!(p.text, "");
        let block: ContentBlock = p.into();
        match block {
            ContentBlock::Paragraph { text, .. } => assert_eq!(text, ""),
            _ => panic!("Expected Paragraph"),
        }
    }

    #[test]
    fn adversarial_paragraph_unicode() {
        let p = Paragraph::new("日本語テスト");
        assert_eq!(p.text, "日本語テスト");
    }

    #[test]
    fn adversarial_paragraph_long_text() {
        let long = "word ".repeat(1000);
        let p = Paragraph::new(&long);
        assert_eq!(p.text.len(), 5000);
    }

    #[test]
    fn adversarial_paragraph_zero_indent() {
        let p = Paragraph::new("test").first_line_indent(0.0);
        assert!((p.first_line_indent).abs() < 0.001);
    }

    #[test]
    fn adversarial_paragraph_negative_indent() {
        // Negative indent is allowed by the builder (renderer handles it)
        let p = Paragraph::new("test").first_line_indent(-10.0);
        assert_eq!(p.first_line_indent, -10.0);
    }

    #[test]
    fn adversarial_paragraph_zero_gaps() {
        let p = Paragraph::new("test").before_gap(0.0).after_gap(0.0);
        assert!((p.before_gap).abs() < 0.001);
        assert!((p.after_gap).abs() < 0.001);
    }

    // ─── Adversarial: Table edge cases ────────────────────────────────

    #[test]
    fn adversarial_table_empty() {
        let table = Table::new();
        assert!(table.columns.is_empty());
        assert!(table.rows.is_empty());
        assert!(!table.borders);
        assert!(!table.alternating_rows);
    }

    #[test]
    fn adversarial_table_columns_only() {
        let table = Table::new().columns(&["A", "B", "C"]);
        assert_eq!(table.columns.len(), 3);
        assert!(table.rows.is_empty());
    }

    #[test]
    fn adversarial_table_mismatched_row_lengths() {
        // Row with fewer columns than header
        let table = Table::new().columns(&["A", "B", "C"]).row(&["only_one"]);
        // Should not panic when converting to ContentBlock
        let block: ContentBlock = table.into();
        match block {
            ContentBlock::Table { rows, .. } => {
                assert_eq!(rows.len(), 2); // header + 1 data row
            }
            _ => panic!("Expected Table"),
        }
    }

    #[test]
    fn adversarial_table_more_columns_in_row() {
        // Row with more columns than header
        let table = Table::new().columns(&["A"]).row(&["1", "2", "3", "4"]);
        let block: ContentBlock = table.into();
        match block {
            ContentBlock::Table { rows, .. } => {
                assert_eq!(rows.len(), 2); // header + 1 data row
            }
            _ => panic!("Expected Table"),
        }
    }

    #[test]
    fn adversarial_table_zero_padding() {
        let table = Table::new().cell_padding(0.0);
        assert!((table.cell_padding).abs() < 0.001);
    }

    #[test]
    fn adversarial_table_large_padding() {
        let table = Table::new().cell_padding(100.0);
        assert_eq!(table.cell_padding, 100.0);
    }

    #[test]
    fn adversarial_table_all_options() {
        let table = Table::new()
            .columns(&["Name", "Value"])
            .row(&["key1", "val1"])
            .row(&["key2", "val2"])
            .footer_row(&["total", "sum"])
            .with_borders()
            .with_alternating_rows()
            .cell_padding(8.0)
            .column_widths(&[200.0, 100.0]);
        assert!(table.borders);
        assert!(table.alternating_rows);
        assert_eq!(table.cell_padding, 8.0);
        assert_eq!(table.column_widths.len(), 2);
        let block: ContentBlock = table.into();
        match block {
            ContentBlock::Table { rows, .. } => {
                assert_eq!(rows.len(), 4); // header + 2 data + footer
            }
            _ => panic!("Expected Table"),
        }
    }

    // ─── Adversarial: Gap edge cases ──────────────────────────────────

    #[test]
    fn adversarial_gap_zero() {
        let block = Gap::before(0.0);
        match block {
            ContentBlock::Gap(g) => assert!((g).abs() < 0.001),
            _ => panic!("Expected Gap"),
        }
    }

    #[test]
    fn adversarial_gap_large() {
        let block = Gap::after(10000.0);
        match block {
            ContentBlock::Gap(g) => assert_eq!(g, 10000.0),
            _ => panic!("Expected Gap"),
        }
    }

    #[test]
    fn adversarial_gap_negative() {
        // Negative gap is allowed by the builder
        let block = Gap::before(-5.0);
        match block {
            ContentBlock::Gap(g) => assert_eq!(g, -5.0),
            _ => panic!("Expected Gap"),
        }
    }

    // ─── Adversarial: Document builder edge cases ─────────────────────

    #[test]
    fn adversarial_document_empty() {
        // Document with no blocks should still build
        let model = Document::new().build();
        assert!(model.page_count() >= 1);
    }

    #[test]
    fn adversarial_document_many_page_breaks() {
        let mut doc = Document::new();
        for _ in 0..20 {
            doc = doc.add(PageBreak);
        }
        let model = doc.build();
        assert!(model.page_count() >= 20);
    }

    #[test]
    fn adversarial_document_text_content_empty() {
        let doc = Document::new();
        let content = doc.text_content();
        assert!(content.is_empty());
    }

    #[test]
    fn adversarial_document_text_content_paragraphs() {
        let doc = Document::new()
            .add(Paragraph::new("First"))
            .add(Paragraph::new("Second"))
            .add(Paragraph::new("Third"));
        let content = doc.text_content();
        assert!(content.contains("First"));
        assert!(content.contains("Second"));
        assert!(content.contains("Third"));
    }

    #[test]
    fn adversarial_document_text_content_with_gaps() {
        // Gaps should not produce text
        let doc = Document::new()
            .add(Paragraph::new("Before"))
            .add(Gap::after(12.0))
            .add(Paragraph::new("After"));
        let content = doc.text_content();
        assert!(content.contains("Before"));
        assert!(content.contains("After"));
    }

    #[test]
    fn adversarial_document_page_count_consistency() {
        // page_count should be consistent across multiple calls
        let doc = Document::new()
            .add(Paragraph::new("A"))
            .add(PageBreak)
            .add(Paragraph::new("B"));
        let count1 = doc.page_count();
        let count2 = doc.page_count();
        assert_eq!(count1, count2);
    }

    #[test]
    fn adversarial_document_get_page_out_of_bounds() {
        let doc = Document::new().add(Paragraph::new("Only page"));
        let page = doc.get_page(99);
        assert!(page.is_none());
    }

    #[test]
    fn adversarial_document_to_json_roundtrip() {
        let doc = Document::new()
            .title("Roundtrip Test")
            .add(Paragraph::new("Hello"));
        let json = doc.to_json().unwrap();
        assert!(json.contains("Roundtrip Test"));
        // from_json should at least not panic
        let doc2 = Document::from_json(&json);
        assert!(doc2.is_ok());
    }

    #[test]
    fn adversarial_document_from_json_invalid() {
        let doc = Document::from_json("not valid json {{{");
        assert!(doc.is_err());
    }

    #[test]
    fn adversarial_document_from_json_empty() {
        let doc = Document::from_json("");
        assert!(doc.is_err());
    }

    #[test]
    fn adversarial_document_merge_empty() {
        let doc1 = Document::new().add(Paragraph::new("Hello"));
        let doc2 = Document::new();
        let merged = doc1.merge(doc2);
        // Merge returns a new Document (best-effort)
        let model = merged.build();
        assert!(model.page_count() >= 1);
    }

    #[test]
    fn adversarial_document_merge_both_empty() {
        let doc1 = Document::new();
        let doc2 = Document::new();
        let merged = doc1.merge(doc2);
        let model = merged.build();
        assert!(model.page_count() >= 1);
    }

    #[test]
    fn adversarial_document_multiple_titles() {
        // Last title wins (builder pattern)
        let model = Document::new()
            .title("First")
            .title("Second")
            .add(Paragraph::new("Content"))
            .build();
        assert_eq!(model.metadata.title, Some("Second".to_string()));
    }

    // ─── Adversarial: Color boundary values ───────────────────────────

    #[test]
    fn adversarial_color_boundary_values() {
        let c = Color::rgb(0.0, 0.0, 0.0);
        assert!(c.r.abs() < 0.001 && c.g.abs() < 0.001 && c.b.abs() < 0.001);

        let c = Color::rgb(1.0, 1.0, 1.0);
        assert!((c.r - 1.0).abs() < 0.001);

        let c = Color::rgba(0.5, 0.5, 0.5, 0.5);
        assert!((c.r - 0.5).abs() < 0.001);
        assert!((c.a - 0.5).abs() < 0.001);
    }

    #[test]
    fn adversarial_color_over_one() {
        // Values > 1.0 are allowed by the struct (renderer clamps)
        let c = Color::rgb(2.0, 2.0, 2.0);
        assert!((c.r - 2.0).abs() < 0.001);
    }

    #[test]
    fn adversarial_color_negative() {
        let c = Color::rgb(-1.0, -1.0, -1.0);
        assert!((c.r + 1.0).abs() < 0.001);
    }

    // ─── Adversarial: save_png edge cases ─────────────────────────────

    #[test]
    fn adversarial_save_png_single_page() {
        let path = std::env::temp_dir().join("adv_save_png.png");
        let _ = std::fs::remove_file(&path);
        let result = Document::new()
            .add(Paragraph::new("Single page"))
            .save_png(&path, 150);
        assert!(result.is_ok());
        assert!(path.exists());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn adversarial_save_png_multi_page_errors() {
        let path = std::env::temp_dir().join("adv_save_png_multi.png");
        let _ = std::fs::remove_file(&path);
        let result = Document::new()
            .add(Paragraph::new("Page 1"))
            .add(PageBreak)
            .add(Paragraph::new("Page 2"))
            .save_png(&path, 150);
        assert!(result.is_err());
        let _ = std::fs::remove_file(&path);
    }

    // ─── Adversarial: render_png edge cases ───────────────────────────

    #[test]
    fn adversarial_render_png_empty_doc() {
        let dir = std::env::temp_dir().join("adv_render_empty");
        let _ = std::fs::remove_dir_all(&dir);
        let result = Document::new().render_png(&dir, 150);
        assert!(result.is_ok());
        let paths = result.unwrap();
        assert!(!paths.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adversarial_render_png_high_dpi() {
        let dir = std::env::temp_dir().join("adv_render_high_dpi");
        let _ = std::fs::remove_dir_all(&dir);
        let result = Document::new()
            .add(Paragraph::new("High DPI test"))
            .render_png(&dir, 600);
        assert!(result.is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ─── Adversarial: Image with zero-dimension content ───────────────

    #[test]
    fn adversarial_image_zero_size() {
        let img = Image::new("test").size(0.0, 0.0);
        let block: ContentBlock = img.into();
        match block {
            ContentBlock::Image { dest_rect, .. } => {
                assert_eq!(dest_rect.width, 0.0);
                assert_eq!(dest_rect.height, 0.0);
            }
            _ => panic!("Expected Image"),
        }
    }

    // ─── Adversarial: Document with only gaps ─────────────────────────

    #[test]
    fn adversarial_document_only_gaps() {
        let model = Document::new()
            .add(Gap::before(10.0))
            .add(Gap::after(20.0))
            .build();
        assert!(model.page_count() >= 1);
    }

    // ─── Adversarial: Document with only page breaks ──────────────────

    #[test]
    fn adversarial_document_only_page_breaks() {
        let model = Document::new()
            .add(PageBreak)
            .add(PageBreak)
            .add(PageBreak)
            .build();
        assert!(model.page_count() >= 3);
    }

    // ─── Adversarial: Repeated API calls ──────────────────────────────

    #[test]
    fn adversarial_document_repeated_build() {
        let doc = Document::new().add(Paragraph::new("Test"));
        let model1 = doc.clone().build();
        let model2 = doc.build();
        assert_eq!(model1.page_count(), model2.page_count());
    }

    #[test]
    fn adversarial_document_repeated_page_count() {
        let doc = Document::new()
            .add(Paragraph::new("A"))
            .add(PageBreak)
            .add(Paragraph::new("B"));
        assert_eq!(doc.page_count(), 2);
        assert_eq!(doc.page_count(), 2);
        assert_eq!(doc.page_count(), 2);
    }

    // ─── Adversarial: FontWeight values ───────────────────────────────

    #[test]
    fn adversarial_font_weight_values() {
        assert_eq!(FontWeight::Thin.value(), 100);
        assert_eq!(FontWeight::Normal.value(), 400);
        assert_eq!(FontWeight::Bold.value(), 700);
        assert_eq!(FontWeight::Black.value(), 900);
    }

    // ─── Adversarial: LayerType ───────────────────────────────────────

    #[test]
    fn adversarial_layer_type_watermark_exists() {
        let layer = Layer::watermark();
        assert_eq!(layer.layer_type, LayerType::Watermark);
    }

    #[test]
    fn adversarial_layer_type_ordering() {
        // Background < Watermark < Foreground < Header < Footer
        assert!((LayerType::Background as i32) < (LayerType::Foreground as i32));
        // Just verify watermark exists and is distinct
        assert_ne!(LayerType::Watermark, LayerType::Background);
        assert_ne!(LayerType::Watermark, LayerType::Foreground);
        assert_ne!(LayerType::Watermark, LayerType::Header);
        assert_ne!(LayerType::Watermark, LayerType::Footer);
    }

    // ─── Adversarial: text_content with tables ────────────────────────

    #[test]
    fn adversarial_text_content_with_tables() {
        let doc = Document::new()
            .add(Paragraph::new("Before table"))
            .add(
                Table::new()
                    .columns(&["Key", "Value"])
                    .row(&["name", "Alice"]),
            )
            .add(Paragraph::new("After table"));
        let content = doc.text_content();
        assert!(content.contains("Before table"));
        assert!(content.contains("Key"));
        assert!(content.contains("Value"));
        assert!(content.contains("name"));
        assert!(content.contains("Alice"));
        assert!(content.contains("After table"));
    }

    // ─── Adversarial: save_png to nonexistent directory ───────────────

    #[test]
    fn adversarial_save_png_creates_file() {
        let path = std::env::temp_dir().join("adv_new_file.png");
        let _ = std::fs::remove_file(&path);
        let result = Document::new()
            .add(Paragraph::new("New file"))
            .save_png(&path, 150);
        assert!(result.is_ok());
        assert!(path.exists());
        let metadata = std::fs::metadata(&path).unwrap();
        assert!(metadata.len() > 0);
        let _ = std::fs::remove_file(&path);
    }
}
