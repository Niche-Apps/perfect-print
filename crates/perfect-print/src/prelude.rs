//! Prelude: import everything you need with `use perfect_print::prelude::*;`

pub use crate::Color;
pub use crate::ContentBlock;
pub use crate::Document;
pub use crate::DocumentBuilder;
pub use crate::DrawCommand;
pub use crate::FlowConfig;
pub use crate::FlowLayoutEngine;
pub use crate::FontCache;
pub use crate::FontRef;
pub use crate::FontStyle;
pub use crate::FontWeight;
pub use crate::Margins;
pub use crate::PageSize;
pub use crate::Paragraph;
pub use crate::PdfRenderer;
pub use crate::PrintSettings;
pub use crate::Render;
pub use crate::TextStyle;
pub use crate::TinySkiaRenderer;

// Re-export convenience types that don't exist yet
// These will be defined in this module
