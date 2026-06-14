use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::font::FontRef;
use crate::units::{Point, Rect};

/// Text alignment within its bounding area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TextAlign {
    Left,
    Center,
    Right,
    Justified,
}

/// Line cap style for paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}

/// Line join style for paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LineJoin {
    Miter,
    Round,
    Bevel,
}

/// Fill rule for paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

/// Complete style for text rendering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    pub font: FontRef,
    pub size: f64,
    pub color: Color,
    pub align: TextAlign,
    pub line_height: Option<f64>, // None = auto (1.2x font size)
    pub letter_spacing: Option<f64>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

impl TextStyle {
    pub fn new(font: FontRef, size: f64) -> Self {
        Self {
            font,
            size,
            color: Color::black(),
            align: TextAlign::Left,
            line_height: None,
            letter_spacing: None,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
        }
    }
}

/// A shaped glyph - output from rustybuzz shaping, positioned on the page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapedGlyph {
    pub glyph_id: u32,
    pub x_offset: f64,
    pub y_offset: f64,
    pub x_advance: f64,
    pub y_advance: f64,
    pub font_index: usize,
    pub cluster: u32,
}

/// A run of shaped text positioned on the page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    pub text: String,
    pub glyphs: Vec<ShapedGlyph>,
    pub style: TextStyle,
}

/// Transform matrix (2D affine).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub a: f64, // scale x
    pub b: f64, // skew y
    pub c: f64, // skew x
    pub d: f64, // scale y
    pub e: f64, // translate x
    pub f: f64, // translate y
}

impl Transform {
    pub fn identity() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }

    pub fn translate(tx: f64, ty: f64) -> Self {
        Self {
            e: tx,
            f: ty,
            ..Self::identity()
        }
    }

    pub fn scale(sx: f64, sy: f64) -> Self {
        Self {
            a: sx,
            d: sy,
            ..Self::identity()
        }
    }

    pub fn rotate(angle_rad: f64) -> Self {
        Self {
            a: angle_rad.cos(),
            b: angle_rad.sin(),
            c: -angle_rad.sin(),
            d: angle_rad.cos(),
            e: 0.0,
            f: 0.0,
        }
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::identity()
    }
}

/// A path operation (for vector drawing).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PathOp {
    MoveTo(Point),
    LineTo(Point),
    CurveTo { cp1: Point, cp2: Point, end: Point },
    QuadTo { cp: Point, end: Point },
    Close,
}

/// Draw commands - the canonical rendering instructions.
///
/// This enum represents ALL rendering operations. Every backend (PDF, raster,
/// native print) consumes these same commands. No backend may add its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DrawCommand {
    /// Draw a text run at a position.
    Text {
        run: TextRun,
        position: Point,
        /// Optional max width for wrapping; None = no wrap
        max_width: Option<f64>,
    },

    /// Draw a filled rectangle.
    FillRect { rect: Rect, color: Color },

    /// Draw a stroked rectangle.
    StrokeRect {
        rect: Rect,
        color: Color,
        width: f64,
        line_cap: LineCap,
        line_join: LineJoin,
    },

    /// Draw a filled path.
    FillPath {
        ops: Vec<PathOp>,
        fill_rule: FillRule,
        color: Color,
    },

    /// Draw a stroked path.
    StrokePath {
        ops: Vec<PathOp>,
        width: f64,
        line_cap: LineCap,
        line_join: LineJoin,
        miter_limit: f64,
        color: Color,
    },

    /// Draw an image.
    Image {
        image_id: String,
        dest_rect: Rect,
        /// Optional source rectangle for cropping
        source_rect: Option<Rect>,
    },

    /// Push a clip region.
    PushClip { rect: Rect },

    /// Pop the last clip region.
    PopClip,

    /// Push a transform.
    PushTransform { transform: Transform },

    /// Pop the last transform.
    PopTransform,

    /// Push an opacity.
    PushOpacity { opacity: f64 },

    /// Pop the last opacity.
    PopOpacity,

    /// Start a named group.
    BeginGroup { name: Option<String> },

    /// End the current group.
    EndGroup,

    /// Place a nested block (flow layout result).
    Block {
        rect: Rect,
        /// Commands to render within this block
        commands: Box<Vec<DrawCommand>>,
    },
}

impl DrawCommand {
    /// Get the bounding box of this command, if computable.
    pub fn bounds(&self) -> Option<Rect> {
        match self {
            DrawCommand::FillRect { rect, .. } => Some(*rect),
            DrawCommand::StrokeRect { rect, width, .. } => Some(Rect::new(
                rect.x - width / 2.0,
                rect.y - width / 2.0,
                rect.width + width,
                rect.height + width,
            )),
            DrawCommand::Image { dest_rect, .. } => Some(*dest_rect),
            DrawCommand::Block { rect, .. } => Some(*rect),
            _ => None,
        }
    }
}
