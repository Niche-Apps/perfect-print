//! Text layout, flow layout, pagination, tables, headers, footers
//!
//! This crate provides the layout engine for perfect-print. It converts
//! high-level text commands (with raw text and styles) into positioned
//! glyphs that the PDF and raster backends can render directly.
//!
//! # Pipeline
//!
//! ```text
//! Raw Text + TextStyle
//!         │
//!         ▼
//!  FontCache ──► LoadedFont (fontdb + rustybuzz Face)
//!         │
//!         ▼
//!  TextShaper ──► Vec<ShapedGlyph> (glyph IDs + advances)
//!         │
//!         ▼
//!  ParagraphEngine ──► ParagraphLayout (lines of positioned glyphs)
//!         │
//!         ▼
//!  FlowLayoutEngine ──► DocumentModel (paginated pages)
//! ```

pub mod flow;
pub mod font_loader;
pub mod paragraph;
pub mod table;
pub mod text_shaper;

pub use flow::*;
pub use font_loader::{
    default_fallbacks, FallbackFont, FontCache, FontLoader, FontProperties, LoadedFont,
    SystemFontLoader,
};
pub use paragraph::{
    EnglishHyphenator, Line, ParagraphConfig, ParagraphEngine, ParagraphLayout, PositionedGlyph,
};
pub use table::*;
pub use text_shaper::TextShaper;
