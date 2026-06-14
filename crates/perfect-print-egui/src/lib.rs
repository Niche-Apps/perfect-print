//! egui integration for perfect-print.
//!
//! Provides a renderer that can display document pages as egui images.
//!
//! ## Example
//!
//! ```no_run
//! use perfect_print_core::page::PageSize;
//! use perfect_print_egui::EguiRenderer;
//!
//! // Build a document model using the core API
//! let model = perfect_print_core::document::DocumentBuilder::new()
//!     .page(PageSize::Letter)
//!     .build()
//!     .unwrap();
//!
//! let renderer = EguiRenderer::new().dpi(150.0);
//! let pages = renderer.render_pages(&model);
//! ```

use perfect_print_core::document::DocumentModel;
use perfect_print_core::units::Dpi;
use perfect_print_render::{Render, RenderError, TinySkiaRenderer};

/// Renderer that produces egui-compatible images from document models.
#[cfg(feature = "egui")]
pub struct EguiRenderer {
    dpi: f64,
}

#[cfg(feature = "egui")]
impl EguiRenderer {
    /// Create a new egui renderer at 150 DPI.
    pub fn new() -> Self {
        Self { dpi: 150.0 }
    }

    /// Set the DPI for rendering (default 150).
    pub fn dpi(mut self, dpi: f64) -> Self {
        self.dpi = dpi;
        self
    }

    /// Render all pages as egui `ColorImage`s.
    pub fn render_pages(
        &self,
        model: &DocumentModel,
    ) -> Result<Vec<egui::ColorImage>, RenderError> {
        let skia_renderer = TinySkiaRenderer::new();
        let pngs = skia_renderer.render_to_raster(model, Dpi(self.dpi), &std::env::temp_dir())?;

        let mut images = Vec::new();
        for png_path in &pngs {
            let img = image::open(png_path).map_err(|e| RenderError::TinySkia(e.to_string()))?;

            let rgba = img.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            let pixels: Vec<u8> = rgba.into_raw();
            images.push(egui::ColorImage::from_rgba_unmultiplied(size, &pixels));
        }

        Ok(images)
    }
}

#[cfg(feature = "egui")]
impl Default for EguiRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Stub for non-egui builds.
#[cfg(not(feature = "egui"))]
pub struct EguiRenderer;

#[cfg(not(feature = "egui"))]
impl EguiRenderer {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(not(feature = "egui"))]
impl Default for EguiRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Render a document model to a single egui `ColorImage` (first page only).
#[cfg(feature = "egui")]
pub fn render_page(model: &DocumentModel, dpi: f64) -> Result<egui::ColorImage, RenderError> {
    let renderer = EguiRenderer::new().dpi(dpi);
    let mut pages = renderer.render_pages(model)?;
    if pages.is_empty() {
        return Err(RenderError::InvalidPageIndex(0));
    }
    Ok(pages.remove(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renderer_default() {
        let _renderer = EguiRenderer::new();
    }

    #[test]
    fn test_renderer_dpi() {
        let renderer = EguiRenderer::new().dpi(300.0);
        assert_eq!(renderer.dpi, 300.0);
    }
}
