use serde::{Deserialize, Serialize};

use crate::units::Size;

/// Standard page sizes in points (1/72 inch).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PageSize {
    // North American
    Letter,  // 8.5 x 11 in = 612 x 792 pt
    Legal,   // 8.5 x 14 in = 612 x 1008 pt
    Tabloid, // 11 x 17 in = 792 x 1224 pt
    Ledger,  // 17 x 11 in = 1224 x 792 pt

    // ISO A series
    A0,
    A1,
    A2,
    A3,
    A4,
    A5,
    A6,

    // ISO B series
    B0,
    B1,
    B2,
    B3,
    B4,
    B5,

    // Custom
    Custom {
        width: f64,
        height: f64,
    },

    /// Roll paper (receipt) mode — fixed width, unlimited height.
    /// The page grows vertically to fit all content.
    RollPaper {
        width: f64,
    },
}

impl PageSize {
    /// Get the page size in points.
    pub fn to_size(self) -> Size {
        match self {
            PageSize::Letter => Size::new(612.0, 792.0),
            PageSize::Legal => Size::new(612.0, 1008.0),
            PageSize::Tabloid => Size::new(792.0, 1224.0),
            PageSize::Ledger => Size::new(1224.0, 792.0),
            PageSize::A0 => Size::new(2384.0, 3370.0),
            PageSize::A1 => Size::new(1684.0, 2384.0),
            PageSize::A2 => Size::new(1191.0, 1684.0),
            PageSize::A3 => Size::new(842.0, 1191.0),
            PageSize::A4 => Size::new(595.0, 842.0),
            PageSize::A5 => Size::new(420.0, 595.0),
            PageSize::A6 => Size::new(298.0, 420.0),
            PageSize::B0 => Size::new(2920.0, 4127.0),
            PageSize::B1 => Size::new(2064.0, 2920.0),
            PageSize::B2 => Size::new(1460.0, 2064.0),
            PageSize::B3 => Size::new(1032.0, 1460.0),
            PageSize::B4 => Size::new(729.0, 1032.0),
            PageSize::B5 => Size::new(516.0, 729.0),
            PageSize::Custom { width, height } => Size::new(width, height),
            PageSize::RollPaper { width } => Size::new(width, f64::MAX),
        }
    }

    /// Get page width in points.
    pub fn width(self) -> f64 {
        self.to_size().width
    }

    /// Get page height in points.
    /// For roll paper, returns `f64::MAX`.
    pub fn height(self) -> f64 {
        self.to_size().height
    }

    /// Returns true if this is a roll paper (receipt) page size.
    pub fn is_roll_paper(self) -> bool {
        matches!(self, PageSize::RollPaper { .. })
    }
}

/// Page margins in points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Margins {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

impl Margins {
    pub fn all(margin: f64) -> Self {
        Self {
            top: margin,
            right: margin,
            bottom: margin,
            left: margin,
        }
    }

    pub fn symmetric(vertical: f64, horizontal: f64) -> Self {
        Self {
            top: vertical,
            bottom: vertical,
            left: horizontal,
            right: horizontal,
        }
    }

    pub fn new(top: f64, right: f64, bottom: f64, left: f64) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }
}

impl Default for Margins {
    fn default() -> Self {
        Self::all(72.0) // 1 inch default
    }
}

/// Layer type determines z-ordering and behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LayerType {
    Background,
    Watermark,
    Foreground,
    Header,
    Footer,
}

/// A layer on a page containing draw commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub layer_type: LayerType,
    pub commands: Vec<crate::draw::DrawCommand>,
}

impl Layer {
    pub fn new(layer_type: LayerType) -> Self {
        Self {
            layer_type,
            commands: Vec::new(),
        }
    }

    pub fn foreground() -> Self {
        Self::new(LayerType::Foreground)
    }

    pub fn background() -> Self {
        Self::new(LayerType::Background)
    }

    pub fn header() -> Self {
        Self::new(LayerType::Header)
    }

    pub fn footer() -> Self {
        Self::new(LayerType::Footer)
    }

    pub fn watermark() -> Self {
        Self::new(LayerType::Watermark)
    }
}

/// A single page in the document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub size: Size,
    pub margins: Margins,
    pub layers: Vec<Layer>,
}

impl Page {
    pub fn new(size: PageSize) -> Self {
        Self {
            size: size.to_size(),
            margins: Margins::default(),
            layers: vec![Layer::foreground()],
        }
    }

    pub fn size(mut self, size: PageSize) -> Self {
        self.size = size.to_size();
        self
    }

    pub fn margins(mut self, margins: Margins) -> Self {
        self.margins = margins;
        self
    }

    pub fn margin(mut self, margin: f64) -> Self {
        self.margins = Margins::all(margin);
        self
    }

    pub fn add_layer(mut self, layer: Layer) -> Self {
        self.layers.push(layer);
        self
    }

    pub fn foreground(&mut self) -> &mut Layer {
        // Return the foreground layer (create if needed)
        if let Some(idx) = self
            .layers
            .iter()
            .position(|l| l.layer_type == LayerType::Foreground)
        {
            &mut self.layers[idx]
        } else {
            self.layers.push(Layer::foreground());
            self.layers.last_mut().unwrap()
        }
    }

    /// Get the content area (page size minus margins).
    pub fn content_rect(&self) -> crate::units::Rect {
        use crate::units::Rect;
        Rect::new(
            self.margins.left,
            self.margins.top,
            self.size.width - self.margins.left - self.margins.right,
            self.size.height - self.margins.top - self.margins.bottom,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn letter_size() {
        let size = PageSize::Letter.to_size();
        assert_relative_eq!(size.width, 612.0);
        assert_relative_eq!(size.height, 792.0);
    }

    #[test]
    fn a4_size() {
        let size = PageSize::A4.to_size();
        assert_relative_eq!(size.width, 595.0, epsilon = 0.5);
        assert_relative_eq!(size.height, 842.0, epsilon = 0.5);
    }

    #[test]
    fn content_rect() {
        let page = Page::new(PageSize::Letter).margin(72.0);
        let rect = page.content_rect();
        assert_relative_eq!(rect.x, 72.0);
        assert_relative_eq!(rect.y, 72.0);
        assert_relative_eq!(rect.width, 612.0 - 144.0);
        assert_relative_eq!(rect.height, 792.0 - 144.0);
    }

    #[test]
    fn page_serialization_stability() {
        // Pages must serialize to stable JSON (same output every time)
        let page1 = Page::new(PageSize::Letter).margin(72.0);
        let page2 = Page::new(PageSize::Letter).margin(72.0);

        let json1 = serde_json::to_string(&page1).unwrap();
        let json2 = serde_json::to_string(&page2).unwrap();

        assert_eq!(json1, json2, "Identical pages must produce identical JSON");
    }

    #[test]
    fn watermark_layer() {
        let layer = Layer::watermark();
        assert_eq!(layer.layer_type, LayerType::Watermark);
        assert!(layer.commands.is_empty());
    }
}
