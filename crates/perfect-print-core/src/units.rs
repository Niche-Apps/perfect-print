use serde::{Deserialize, Serialize};

/// A length value with an explicit unit.
///
/// This is the fundamental measurement type in perfect-print.
/// All layout computations are done in points (1/72 inch) internally.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Length {
    pub value: f64,
    pub unit: LengthUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LengthUnit {
    /// PostScript points (1/72 inch). The internal canonical unit.
    Points,
    /// Inches
    Inches,
    /// Millimeters
    Mm,
    /// Pixels at a specific DPI
    Px,
}

impl Length {
    /// Create a length in points.
    pub fn points(value: f64) -> Self {
        Self {
            value,
            unit: LengthUnit::Points,
        }
    }

    /// Create a length in inches.
    pub fn inches(value: f64) -> Self {
        Self {
            value,
            unit: LengthUnit::Inches,
        }
    }

    /// Create a length in millimeters.
    pub fn mm(value: f64) -> Self {
        Self {
            value,
            unit: LengthUnit::Mm,
        }
    }

    /// Create a length in pixels at the given DPI.
    pub fn px(value: f64, dpi: f64) -> Self {
        Self {
            value: value / dpi * 72.0,
            unit: LengthUnit::Px,
        }
    }

    /// Convert to points (the canonical internal unit).
    pub fn to_points(self) -> f64 {
        match self.unit {
            LengthUnit::Points => self.value,
            LengthUnit::Inches => self.value * 72.0,
            LengthUnit::Mm => self.value * 72.0 / 25.4,
            LengthUnit::Px => self.value, // already converted in constructor
        }
    }

    /// Convert from points to the given unit.
    pub fn from_points(points: f64, unit: LengthUnit) -> Self {
        let value = match unit {
            LengthUnit::Points => points,
            LengthUnit::Inches => points / 72.0,
            LengthUnit::Mm => points * 25.4 / 72.0,
            LengthUnit::Px => points, // caller must apply DPI
        };
        Self { value, unit }
    }

    /// Convert to inches.
    pub fn to_inches(self) -> f64 {
        self.to_points() / 72.0
    }

    /// Convert to millimeters.
    pub fn to_mm(self) -> f64 {
        self.to_points() * 25.4 / 72.0
    }

    /// Convert to pixels at the given DPI.
    pub fn to_px(self, dpi: f64) -> f64 {
        self.to_points() * dpi / 72.0
    }
}

/// A 2D point in points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn zero() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

/// A 2D size in points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

impl Size {
    pub fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

/// A rectangle in points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn from_points(origin: Point, size: Size) -> Self {
        Self::new(origin.x, origin.y, size.width, size.height)
    }

    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.x
            && point.x <= self.right()
            && point.y >= self.y
            && point.y <= self.bottom()
    }
}

/// Dots per inch - used for raster output and px conversions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Dpi(pub f64);

impl Dpi {
    pub const SCREEN: Dpi = Dpi(72.0);
    pub const PRINT_LOW: Dpi = Dpi(150.0);
    pub const PRINT_STANDARD: Dpi = Dpi(300.0);
    pub const PRINT_HIGH: Dpi = Dpi(600.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn points_identity() {
        let l = Length::points(72.0);
        assert_relative_eq!(l.to_points(), 72.0);
    }

    #[test]
    fn inches_to_points() {
        let l = Length::inches(1.0);
        assert_relative_eq!(l.to_points(), 72.0, epsilon = 0.001);
    }

    #[test]
    fn mm_to_points() {
        let l = Length::mm(25.4);
        assert_relative_eq!(l.to_points(), 72.0, epsilon = 0.001);
    }

    #[test]
    fn px_to_points() {
        let l = Length::px(300.0, 300.0);
        assert_relative_eq!(l.to_points(), 72.0, epsilon = 0.001);
    }

    #[test]
    fn points_to_inches() {
        let l = Length::points(144.0);
        assert_relative_eq!(l.to_inches(), 2.0, epsilon = 0.001);
    }

    #[test]
    fn points_to_mm() {
        let l = Length::points(72.0);
        assert_relative_eq!(l.to_mm(), 25.4, epsilon = 0.001);
    }

    #[test]
    fn round_trip_inches() {
        let original = 8.5;
        let l = Length::inches(original);
        let pts = l.to_points();
        let back = Length::from_points(pts, LengthUnit::Inches);
        assert_relative_eq!(back.value, original, epsilon = 0.0001);
    }

    #[test]
    fn round_trip_mm() {
        let original = 210.0;
        let l = Length::mm(original);
        let pts = l.to_points();
        let back = Length::from_points(pts, LengthUnit::Mm);
        assert_relative_eq!(back.value, original, epsilon = 0.0001);
    }

    #[test]
    fn rect_contains() {
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        assert!(r.contains(Point::new(50.0, 40.0)));
        assert!(!r.contains(Point::new(5.0, 40.0)));
        assert!(!r.contains(Point::new(50.0, 100.0)));
    }
}
