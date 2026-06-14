//! iced integration for perfect-print.
//!
//! Provides a renderer that can display document pages as iced images.
//!
//! ## Example
//!
//! ```no_run
//! use perfect_print_core::page::PageSize;
//! use perfect_print_iced::IcedRenderer;
//!
//! let model = perfect_print_core::document::DocumentBuilder::new()
//!     .page(PageSize::Letter)
//!     .build()
//!     .unwrap();
//!
//! let renderer = IcedRenderer::new().dpi(150.0);
//! let pages = renderer.render_pages(&model);
//! ```

use perfect_print_core::document::DocumentModel;
use perfect_print_core::units::Dpi;
use perfect_print_render::{Render, RenderError, TinySkiaRenderer};

/// Renderer that produces iced-compatible images from document models.
#[cfg(feature = "iced")]
pub struct IcedRenderer {
    dpi: f64,
}

#[cfg(feature = "iced")]
impl IcedRenderer {
    /// Create a new iced renderer at 150 DPI.
    pub fn new() -> Self {
        Self { dpi: 150.0 }
    }

    /// Set the DPI for rendering (default 150).
    pub fn dpi(mut self, dpi: f64) -> Self {
        self.dpi = dpi;
        self
    }

    /// Render all pages as iced `Handle`s.
    pub fn render_pages(
        &self,
        model: &DocumentModel,
    ) -> Result<Vec<iced::widget::image::Handle>, RenderError> {
        let skia_renderer = TinySkiaRenderer::new();
        let pngs = skia_renderer.render_to_raster(model, Dpi(self.dpi), &std::env::temp_dir())?;

        let mut handles = Vec::new();
        for png_path in &pngs {
            let img = image::open(png_path).map_err(|e| RenderError::TinySkia(e.to_string()))?;
            let rgba = img.to_rgba8();
            let width = rgba.width();
            let height = rgba.height();
            let pixels = rgba.into_raw();
            let handle = iced::widget::image::Handle::from_rgba(width, height, pixels);
            handles.push(handle);
        }

        Ok(handles)
    }
}

#[cfg(feature = "iced")]
impl Default for IcedRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Stub for non-iced builds.
#[cfg(not(feature = "iced"))]
pub struct IcedRenderer;

#[cfg(not(feature = "iced"))]
impl IcedRenderer {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(not(feature = "iced"))]
impl Default for IcedRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Render a document model to a single iced image handle (first page only).
#[cfg(feature = "iced")]
pub fn render_page(
    model: &DocumentModel,
    dpi: f64,
) -> Result<iced::widget::image::Handle, RenderError> {
    let renderer = IcedRenderer::new().dpi(dpi);
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
        let _renderer = IcedRenderer::new();
    }

    #[test]
    fn test_renderer_dpi() {
        let renderer = IcedRenderer::new().dpi(300.0);
        assert_eq!(renderer.dpi, 300.0);
    }
}
