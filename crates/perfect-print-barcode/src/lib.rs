//! Barcode and QR code generation for perfect-print.
//!
//! Generates barcode and QR code images as `tiny_skia::Pixmap` that can be
//! embedded directly into a `DocumentModel` via `ContentBlock::Image`.
//!
//! # Supported formats
//!
//! ## QR Codes
//! - Model 2 (standard QR)
//! - All 4 error correction levels (L, M, Q, H)
//!
//! ## 1D Barcodes
//! - Code 128
//! - Code 39
//! - EAN-13, EAN-8
//! - UPC-A
//! - Interleaved 2 of 5
//! - Codabar

use perfect_print_core::image::ImageData;
use tiny_skia::{Paint, Pixmap, Transform};

/// QR code error correction level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QrEcLevel {
    /// ~7% recovery
    Low,
    /// ~15% recovery
    Medium,
    /// ~25% recovery
    Quartile,
    /// ~30% recovery
    High,
}

impl QrEcLevel {
    fn to_qrcodegen(self) -> qrcodegen::QrCodeEcc {
        use qrcodegen::QrCodeEcc;
        match self {
            QrEcLevel::Low => QrCodeEcc::Low,
            QrEcLevel::Medium => QrCodeEcc::Medium,
            QrEcLevel::Quartile => QrCodeEcc::Quartile,
            QrEcLevel::High => QrCodeEcc::High,
        }
    }
}

/// QR code builder.
#[derive(Debug, Clone)]
pub struct QrCode {
    text: String,
    ec_level: QrEcLevel,
    border: u8,
}

impl QrCode {
    /// Create a new QR code with the given text content.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ec_level: QrEcLevel::Medium,
            border: 2,
        }
    }

    /// Set the error correction level (default: Medium).
    pub fn ec_level(mut self, level: QrEcLevel) -> Self {
        self.ec_level = level;
        self
    }

    /// Set the border width in modules (default: 2).
    pub fn border(mut self, border: u8) -> Self {
        self.border = border;
        self
    }

    /// Generate the QR code as a `tiny_skia::Pixmap`.
    ///
    /// `module_size` is the size of each module in pixels.
    pub fn render(&self, module_size: u32) -> Result<Pixmap, String> {
        let qr = qrcodegen::QrCode::encode_text(&self.text, self.ec_level.to_qrcodegen())
            .map_err(|e| format!("QR encode error: {}", e))?;

        let size = qr.size();
        let total_size = (size as u32 + 2 * self.border as u32) * module_size;
        let mut pixmap = Pixmap::new(total_size, total_size).ok_or("Failed to create pixmap")?;

        // Fill white background
        pixmap.fill(tiny_skia::Color::WHITE);

        // Draw black modules
        let paint = Paint {
            shader: tiny_skia::Shader::SolidColor(tiny_skia::Color::BLACK),
            ..Default::default()
        };

        for y in 0..size {
            for x in 0..size {
                if qr.get_module(x, y) {
                    let px = (self.border as u32 + x as u32) * module_size;
                    let py = (self.border as u32 + y as u32) * module_size;
                    let rect = tiny_skia::Rect::from_xywh(
                        px as f32,
                        py as f32,
                        module_size as f32,
                        module_size as f32,
                    )
                    .unwrap();
                    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
                }
            }
        }

        Ok(pixmap)
    }

    /// Generate the QR code as an `ImageData` for embedding in a document.
    pub fn to_image_data(&self, module_size: u32) -> Result<ImageData, String> {
        let pixmap = self.render(module_size)?;
        let width = pixmap.width();
        let height = pixmap.height();
        let pixels = pixmap.data().to_vec();
        Ok(ImageData {
            width,
            height,
            pixels,
        })
    }
}

/// 1D barcode symbology.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarcodeSym {
    Code128,
    Code39,
    Ean13,
    Ean8,
    UpcA,
    Interleaved2of5,
    Codabar,
}

/// 1D barcode builder.
#[derive(Debug, Clone)]
pub struct Barcode {
    data: String,
    sym: BarcodeSym,
    height: u32,
    padding: u32,
}

impl Barcode {
    /// Create a new barcode with the given data.
    pub fn new(data: impl Into<String>, sym: BarcodeSym) -> Self {
        Self {
            data: data.into(),
            sym,
            height: 60,
            padding: 10,
        }
    }

    /// Set the barcode height in pixels (default: 60).
    pub fn height(mut self, height: u32) -> Self {
        self.height = height;
        self
    }

    /// Set the padding in pixels (default: 10).
    pub fn padding(mut self, padding: u32) -> Self {
        self.padding = padding;
        self
    }

    /// Generate the barcode as a `tiny_skia::Pixmap`.
    pub fn render(&self) -> Result<Pixmap, String> {
        let encoded = self.encode()?;
        // encoded is Vec<u8> where each byte is b'0' or b'1'
        let bar_count = encoded.len() as u32;
        let width = bar_count + 2 * self.padding;
        let total_height = self.height + 2 * self.padding;
        let mut pixmap = Pixmap::new(width, total_height).ok_or("Failed to create pixmap")?;

        // White background
        pixmap.fill(tiny_skia::Color::WHITE);

        // Draw black bars
        let paint = Paint {
            shader: tiny_skia::Shader::SolidColor(tiny_skia::Color::BLACK),
            ..Default::default()
        };

        for (i, &byte) in encoded.iter().enumerate() {
            if byte == b'1' {
                let x = self.padding + i as u32;
                let rect = tiny_skia::Rect::from_xywh(
                    x as f32,
                    self.padding as f32,
                    1.0,
                    self.height as f32,
                )
                .unwrap();
                pixmap.fill_rect(rect, &paint, Transform::identity(), None);
            }
        }

        Ok(pixmap)
    }

    /// Generate the barcode as an `ImageData` for embedding in a document.
    pub fn to_image_data(&self) -> Result<ImageData, String> {
        let pixmap = self.render()?;
        let width = pixmap.width();
        let height = pixmap.height();
        let pixels = pixmap.data().to_vec();
        Ok(ImageData {
            width,
            height,
            pixels,
        })
    }

    fn encode(&self) -> Result<Vec<u8>, String> {
        use barcoders::sym;

        match self.sym {
            BarcodeSym::Code128 => {
                let builder = sym::code128::Code128::new(&self.data)
                    .map_err(|e| format!("Code128 encode error: {}", e))?;
                Ok(builder.encode())
            }
            BarcodeSym::Code39 => {
                let builder = sym::code39::Code39::new(&self.data)
                    .map_err(|e| format!("Code39 encode error: {}", e))?;
                Ok(builder.encode())
            }
            BarcodeSym::Ean13 => {
                let builder = sym::ean13::EAN13::new(&self.data)
                    .map_err(|e| format!("EAN13 encode error: {}", e))?;
                Ok(builder.encode())
            }
            BarcodeSym::Ean8 => {
                let builder = sym::ean8::EAN8::new(&self.data)
                    .map_err(|e| format!("EAN8 encode error: {}", e))?;
                Ok(builder.encode())
            }
            BarcodeSym::UpcA => {
                let builder = sym::ean13::UPCA::new(&self.data)
                    .map_err(|e| format!("UPCA encode error: {}", e))?;
                Ok(builder.encode())
            }
            BarcodeSym::Interleaved2of5 => {
                let builder = sym::tf::TF::interleaved(&self.data)
                    .map_err(|e| format!("ITF encode error: {}", e))?;
                Ok(builder.encode())
            }
            BarcodeSym::Codabar => {
                let builder = sym::codabar::Codabar::new(&self.data)
                    .map_err(|e| format!("Codabar encode error: {}", e))?;
                Ok(builder.encode())
            }
        }
    }
}

/// Size of a barcode/QR code in points (for layout).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarcodeSize {
    pub width: f64,
    pub height: f64,
}

impl BarcodeSize {
    pub fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }

    /// Calculate the size in points for a QR code.
    pub fn qr_code(module_count: u32, module_size_pts: f64) -> Self {
        let total = module_count as f64 * module_size_pts;
        Self::new(total, total)
    }

    /// Calculate the size in points for a 1D barcode.
    pub fn barcode(bar_count: u32, bar_width_pts: f64, height_pts: f64) -> Self {
        Self::new(bar_count as f64 * bar_width_pts, height_pts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qr_code_basic() {
        let qr = QrCode::new("Hello, World!");
        let pixmap = qr.render(4).expect("QR render failed");
        assert!(pixmap.width() > 0);
        assert!(pixmap.height() > 0);
        assert_eq!(pixmap.width(), pixmap.height()); // QR is square
    }

    #[test]
    fn test_qr_code_ec_levels() {
        for level in [
            QrEcLevel::Low,
            QrEcLevel::Medium,
            QrEcLevel::Quartile,
            QrEcLevel::High,
        ] {
            let qr = QrCode::new("test").ec_level(level);
            let pixmap = qr.render(4).expect("QR render failed");
            assert!(pixmap.width() > 0);
        }
    }

    #[test]
    fn test_qr_code_to_image_data() {
        let qr = QrCode::new("https://example.com");
        let image_data = qr.to_image_data(4).expect("to_image_data failed");
        assert!(image_data.width > 0);
        assert!(image_data.height > 0);
        assert!(!image_data.pixels.is_empty());
    }

    #[test]
    fn test_barcode_code128() {
        // Code 128 requires a start character prefix (À = Code A, Ɓ = Code C)
        let bc = Barcode::new("ÀHELLO", BarcodeSym::Code128);
        let pixmap = bc.render().expect("Code128 render failed");
        assert!(pixmap.width() > 0);
        assert!(pixmap.height() > 0);
    }

    #[test]
    fn test_barcode_code39() {
        let bc = Barcode::new("HELLO", BarcodeSym::Code39);
        let pixmap = bc.render().expect("Code39 render failed");
        assert!(pixmap.width() > 0);
    }

    #[test]
    fn test_barcode_ean13() {
        let bc = Barcode::new("5901234123457", BarcodeSym::Ean13);
        let pixmap = bc.render().expect("EAN13 render failed");
        assert!(pixmap.width() > 0);
    }

    #[test]
    fn test_barcode_to_image_data() {
        let bc = Barcode::new("ÀHELLO", BarcodeSym::Code128);
        let image_data = bc.to_image_data().expect("to_image_data failed");
        assert!(image_data.width > 0);
        assert!(image_data.height > 0);
    }

    #[test]
    fn test_barcode_size() {
        let size = BarcodeSize::qr_code(25, 4.0);
        assert_eq!(size.width, 100.0);
        assert_eq!(size.height, 100.0);

        let size = BarcodeSize::barcode(100, 1.5, 60.0);
        assert_eq!(size.width, 150.0);
        assert_eq!(size.height, 60.0);
    }
}
