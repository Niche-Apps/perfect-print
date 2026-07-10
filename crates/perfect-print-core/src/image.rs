//! Image loading and resource management.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Error type for image operations.
#[derive(Debug, Error)]
pub enum ImageLoadError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Image decode error: {0}")]
    Decode(String),

    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("Image too large: {width}x{height} (max 10000x10000)")]
    TooLarge { width: u32, height: u32 },
}

/// Load an image from a file path (PNG, JPEG, etc.) and return RGBA pixel data.
///
/// # Arguments
/// * `path` — Path to the image file
///
/// # Returns
/// `(width, height, rgba_pixels)` where rgba_pixels is a flat Vec of RGBA bytes.
pub fn load_image(path: &std::path::Path) -> Result<(u32, u32, Vec<u8>), ImageLoadError> {
    let img = image::ImageReader::open(path)
        .map_err(ImageLoadError::Io)?
        .decode()
        .map_err(|e| ImageLoadError::Decode(e.to_string()))?;

    let width = img.width();
    let height = img.height();

    if width > 10_000 || height > 10_000 {
        return Err(ImageLoadError::TooLarge { width, height });
    }

    // Convert to RGBA8
    let rgba = img.to_rgba8();
    let pixels = rgba.into_raw();

    Ok((width, height, pixels))
}

/// Load an image from in-memory bytes.
///
/// Returns `(width, height, rgba_pixels)`.
pub fn load_image_from_bytes(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), ImageLoadError> {
    let img = image::load_from_memory(bytes).map_err(|e| ImageLoadError::Decode(e.to_string()))?;

    let width = img.width();
    let height = img.height();

    if width > 10_000 || height > 10_000 {
        return Err(ImageLoadError::TooLarge { width, height });
    }

    let rgba = img.to_rgba8();
    let pixels = rgba.into_raw();

    Ok((width, height, pixels))
}

/// RGBA pixel data for an image resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageData {
    pub width: u32,
    pub height: u32,
    /// RGBA pixel data, row-major, length = width * height * 4
    pub pixels: Vec<u8>,
}

impl ImageData {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        Self {
            width,
            height,
            pixels,
        }
    }

    /// Load from a file.
    pub fn load(path: &std::path::Path) -> Result<Self, ImageLoadError> {
        let (w, h, pixels) = load_image(path)?;
        Ok(Self::new(w, h, pixels))
    }

    /// Load from bytes.
    pub fn load_from_bytes(bytes: &[u8]) -> Result<Self, ImageLoadError> {
        let (w, h, pixels) = load_image_from_bytes(bytes)?;
        Ok(Self::new(w, h, pixels))
    }

    /// Create a test pattern image (a simple blue gradient).
    pub fn test_pattern(width: u32, height: u32) -> Self {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let r = ((x * 255) / width) as u8;
                let g = ((y * 255) / height) as u8;
                let b = 128_u8;
                let a = 255_u8;
                pixels.extend_from_slice(&[r, g, b, a]);
            }
        }
        Self::new(width, height, pixels)
    }

    /// Get the pixel at (x, y) as (r, g, b, a).
    pub fn pixel(&self, x: u32, y: u32) -> Option<(u8, u8, u8, u8)> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = ((y * self.width + x) * 4) as usize;
        Some((
            self.pixels[offset],
            self.pixels[offset + 1],
            self.pixels[offset + 2],
            self.pixels[offset + 3],
        ))
    }

    /// Encode as PNG bytes.
    pub fn to_png(&self) -> Result<Vec<u8>, ImageLoadError> {
        let mut buf = Vec::new();
        use image::ImageEncoder;
        let encoder = image::codecs::png::PngEncoder::new(&mut buf);
        encoder
            .write_image(
                &self.pixels,
                self.width,
                self.height,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| ImageLoadError::Decode(e.to_string()))?;
        Ok(buf)
    }

    /// Encode as JPEG bytes (no alpha channel, white background).
    pub fn to_jpeg(&self, quality: u8) -> Result<Vec<u8>, ImageLoadError> {
        // JPEG doesn't support alpha, composite onto white
        let mut rgb_buf = Vec::with_capacity((self.width * self.height * 3) as usize);
        for chunk in self.pixels.chunks_exact(4) {
            let a = chunk[3] as f32 / 255.0;
            let r = (chunk[0] as f32 * a + 255.0 * (1.0 - a)).clamp(0.0, 255.0) as u8;
            let g = (chunk[1] as f32 * a + 255.0 * (1.0 - a)).clamp(0.0, 255.0) as u8;
            let b = (chunk[2] as f32 * a + 255.0 * (1.0 - a)).clamp(0.0, 255.0) as u8;
            rgb_buf.extend_from_slice(&[r, g, b]);
        }

        let mut buf = Vec::new();
        let quality = quality.clamp(1, 100);
        use image::ImageEncoder;
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
        encoder
            .write_image(
                &rgb_buf,
                self.width,
                self.height,
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|e| ImageLoadError::Decode(e.to_string()))?;
        Ok(buf)
    }
}

impl PartialEq for ImageData {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width && self.height == other.height && self.pixels == other.pixels
    }
}

impl Eq for ImageData {}

/// Image format for encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ImageFormat {
    Png,
    Jpeg,
    RawRgba,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_creation() {
        let img = ImageData::test_pattern(100, 80);
        assert_eq!(img.width, 100);
        assert_eq!(img.height, 80);
        assert_eq!(img.pixels.len(), 100 * 80 * 4);
    }

    #[test]
    fn test_pixel_access() {
        let img = ImageData::test_pattern(10, 10);
        let pixel = img.pixel(0, 0).unwrap();
        assert_eq!(pixel.0, 0); // r = 0
        assert_eq!(pixel.1, 0); // g = 0
        assert_eq!(pixel.2, 128); // b = 128
        assert_eq!(pixel.3, 255); // a = 255
    }

    #[test]
    fn test_pixel_out_of_bounds() {
        let img = ImageData::test_pattern(10, 10);
        assert!(img.pixel(10, 0).is_none());
        assert!(img.pixel(0, 10).is_none());
    }

    #[test]
    fn test_png_roundtrip() {
        let original = ImageData::test_pattern(50, 50);
        let png_bytes = original.to_png().unwrap();
        assert!(!png_bytes.is_empty());

        let loaded = ImageData::load_from_bytes(&png_bytes).unwrap();
        assert_eq!(loaded.width, original.width);
        assert_eq!(loaded.height, original.height);
        // Lossless roundtrip
        assert_eq!(loaded, original);
    }

    #[test]
    fn test_jpeg_roundtrip() {
        let original = ImageData::test_pattern(50, 50);
        let jpeg_bytes = original.to_jpeg(90).unwrap();
        assert!(!jpeg_bytes.is_empty());

        let loaded = ImageData::load_from_bytes(&jpeg_bytes).unwrap();
        assert_eq!(loaded.width, original.width);
        assert_eq!(loaded.height, original.height);
        // JPEG is lossy, so we don't check pixel equality
    }
}
