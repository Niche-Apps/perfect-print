//! Canonical document model, units, pages, layers, draw commands, styles, resources.
//!
//! This crate defines the single canonical page model that ALL output backends consume.
//! PDF, raster, preview, and native print all render from this same model.
//! No backend may create its own layout or rendering path.

pub mod color;
pub mod document;
pub mod draw;
pub mod error;
pub mod font;
pub mod image;
pub mod page;
pub mod resource;
pub mod units;

// Re-export image types for convenience

pub use color::{CmykColor, Color, GrayColor, RgbColor};
pub use document::{DocumentBuilder, DocumentModel as Document};
pub use draw::{DrawCommand, FillRule, LineCap, LineJoin, TextAlign, TextStyle};
pub use error::{
    CoreError, CoreResult, PrintError, PrintResult, PrintWarning, Strictness, ValidationResult,
};
pub use font::{FontRef, FontStyle, FontWeight};
pub use image::{ImageData, ImageFormat, ImageLoadError};
pub use page::{Layer, LayerType, Margins, Page, PageSize};
pub use resource::{ImageStore, ResourceStore};
pub use units::{Dpi, Length, LengthUnit, Point, Rect, Size};
