use serde::{Deserialize, Serialize};

/// An RGBA color.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Color {
    pub fn rgb(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    pub fn rgba(r: f64, g: f64, b: f64, a: f64) -> Self {
        Self { r, g, b, a }
    }

    pub fn black() -> Self {
        Self::rgb(0.0, 0.0, 0.0)
    }

    pub fn white() -> Self {
        Self::rgb(1.0, 1.0, 1.0)
    }

    pub fn red() -> Self {
        Self::rgb(1.0, 0.0, 0.0)
    }

    pub fn green() -> Self {
        Self::rgb(0.0, 1.0, 0.0)
    }

    pub fn blue() -> Self {
        Self::rgb(0.0, 0.0, 1.0)
    }

    pub fn transparent() -> Self {
        Self::rgba(0.0, 0.0, 0.0, 0.0)
    }

    /// Create a gray color from a single value (0.0 = black, 1.0 = white).
    pub fn gray(v: f64) -> Self {
        Self::rgb(v, v, v)
    }

    /// Create a color from RGB integer values (0-255).
    pub fn from_rgb_u8(r: u8, g: u8, b: u8) -> Self {
        Self::rgb(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0)
    }

    /// Create a color from RGBA integer values (0-255).
    pub fn from_rgba_u8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self::rgba(
            r as f64 / 255.0,
            g as f64 / 255.0,
            b as f64 / 255.0,
            a as f64 / 255.0,
        )
    }

    /// Create a color from a hex string like "#FF0000" or "#FF0000FF".
    /// Returns None if the string is not a valid hex color.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.strip_prefix('#')?;
        match hex.len() {
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Self::from_rgb_u8(r, g, b))
            }
            8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                Some(Self::from_rgba_u8(r, g, b, a))
            }
            _ => None,
        }
    }
}

/// RGB color (convenience type for serialized form).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RgbColor {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}

/// CMYK color for print production.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CmykColor {
    pub c: f64,
    pub m: f64,
    pub y: f64,
    pub k: f64,
}

/// Grayscale color.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GrayColor {
    pub value: f64,
}
