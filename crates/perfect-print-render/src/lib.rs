//! Renderer traits and raster/vector adapters

use perfect_print_core::document::DocumentModel;
use perfect_print_core::units::Dpi;
use std::path::Path;
use thiserror::Error;

mod raster;
pub use raster::TinySkiaRenderer;

/// Error type for rendering operations.
#[derive(Debug, Error)]
pub enum RenderError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid page index: {0}")]
    InvalidPageIndex(usize),

    #[error("Page size mismatch")]
    PageSizeMismatch,

    #[error("Font not found: {0}")]
    FontNotFound(String),

    #[error("Image not found: {0}")]
    ImageNotFound(String),

    #[error("TinySkia error: {0}")]
    TinySkia(String),

    #[error("Render error: {0}")]
    Generation(String),
}

/// Result type for rendering operations.
pub type RenderResult<T> = Result<T, RenderError>;

/// Trait for rendering document models to raster output.
pub trait Render {
    /// Render the entire document to raster images (one per page).
    fn render_to_raster(
        &self,
        document: &DocumentModel,
        dpi: Dpi,
        output_dir: &Path,
    ) -> RenderResult<Vec<std::path::PathBuf>>;

    /// Render a single page to a PNG file.
    fn render_page_to_png(
        &self,
        document: &DocumentModel,
        page_index: usize,
        dpi: Dpi,
        output_path: &Path,
    ) -> RenderResult<()>;

    /// Render a single page to a tiny-skia Pixmap.
    fn render_page_to_pixmap(
        &self,
        document: &DocumentModel,
        page_index: usize,
        dpi: Dpi,
    ) -> RenderResult<tiny_skia::Pixmap>;
}
